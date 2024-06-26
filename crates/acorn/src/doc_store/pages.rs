use std::{
	mem::{self, size_of},
	num::NonZeroU16,
};

use static_assertions::const_assert_eq;
use zerocopy::{AsBytes, FromBytes, FromZeroes};

use crate::{
	files::segment::PAGE_BODY_SIZE,
	page_store::{PageId, ReadPage, WritePage},
	utils::units::B,
};

use super::DatabaseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum PageKind {
	FreelistMeta = 0,
	FreelistBlock = 1,
	Records = 2,
}

impl PageKind {
	fn from(value: u8) -> Option<Self> {
		match value {
			0 => Some(PageKind::FreelistMeta),
			1 => Some(PageKind::FreelistBlock),
			2 => Some(PageKind::Records),
			_ => None,
		}
	}
}

const PAGE_HEADER_SIZE: usize = mem::size_of::<PageKind>();

fn set_page_kind(page: &mut impl WritePage, kind: PageKind) -> Result<(), DatabaseError> {
	page.write(0, &[kind as u8])?;
	Ok(())
}

fn assert_page_kind(page: &impl ReadPage, kind: PageKind) -> Result<(), DatabaseError> {
	let mut byte: [u8; 1] = [0];
	page.read(0, &mut byte)?;
	let received = u8::from_ne_bytes(byte);
	if received != kind as u8 {
		let Some(received) = PageKind::from(received) else {
			return Err(DatabaseError::UnknownPageKind(received));
		};
		return Err(DatabaseError::UnexpectedPageKind {
			expected: kind,
			received,
		});
	}
	Ok(())
}

#[derive(AsBytes, FromZeroes, FromBytes)]
#[repr(C, packed)]
struct PageIdRepr {
	segment_num: u32,
	page_num: u16,
}

impl TryFrom<PageIdRepr> for PageId {
	type Error = DatabaseError;

	fn try_from(value: PageIdRepr) -> Result<Self, DatabaseError> {
		let Some(page_num) = NonZeroU16::new(value.page_num) else {
			return Err(DatabaseError::PageFormat(
				"Found invalid page number '0'!".to_string(),
			));
		};
		Ok(PageId::new(value.segment_num, page_num))
	}
}

impl From<PageId> for PageIdRepr {
	fn from(value: PageId) -> Self {
		Self {
			segment_num: value.segment_num,
			page_num: value.page_num.get(),
		}
	}
}

impl From<PageIdRepr> for Option<PageId> {
	fn from(value: PageIdRepr) -> Self {
		Some(PageId::new(
			value.segment_num,
			NonZeroU16::new(value.page_num)?,
		))
	}
}

impl From<Option<PageId>> for PageIdRepr {
	fn from(value: Option<PageId>) -> Self {
		match value {
			Some(page_id) => page_id.into(),
			None => PageIdRepr::new_zeroed(),
		}
	}
}

pub(super) struct MetaPage<P>(P);

impl<P> MetaPage<P> {
	const FREELIST_HEAD_OFFSET: usize = PAGE_HEADER_SIZE;
	const NEXT_PAGE_ID_OFFSET: usize = Self::FREELIST_HEAD_OFFSET + size_of::<PageIdRepr>();

	pub fn new_unchecked(page: P) -> Self {
		Self(page)
	}
}

impl<P: ReadPage> MetaPage<P> {
	pub fn new(page: P) -> Result<Self, DatabaseError> {
		assert_page_kind(&page, PageKind::FreelistMeta)?;
		Ok(Self::new_unchecked(page))
	}

	pub fn get_freelist_head(&self) -> Result<Option<PageId>, DatabaseError> {
		let mut repr = PageIdRepr::new_zeroed();
		self.0
			.read(Self::FREELIST_HEAD_OFFSET, repr.as_bytes_mut())?;
		Ok(repr.into())
	}

	pub fn get_next_page_id(&self) -> Result<PageId, DatabaseError> {
		let mut repr = PageIdRepr::new_zeroed();
		self.0
			.read(Self::NEXT_PAGE_ID_OFFSET, repr.as_bytes_mut())?;
		repr.try_into()
	}
}

impl<P: WritePage> MetaPage<P> {
	pub fn init(&mut self, next_page_id: PageId) -> Result<(), DatabaseError> {
		set_page_kind(&mut self.0, PageKind::FreelistMeta)?;
		self.set_freelist_head(None)?;
		self.set_next_page_id(next_page_id)?;
		Ok(())
	}

	pub fn set_freelist_head(&mut self, value: Option<PageId>) -> Result<(), DatabaseError> {
		let repr = PageIdRepr::from(value);
		self.0.write(Self::FREELIST_HEAD_OFFSET, repr.as_bytes())?;
		Ok(())
	}

	pub fn set_next_page_id(&mut self, value: PageId) -> Result<(), DatabaseError> {
		let repr = PageIdRepr::from(value);
		self.0.write(Self::NEXT_PAGE_ID_OFFSET, repr.as_bytes())?;
		Ok(())
	}
}

pub(super) struct FreelistPage<P>(P);

impl<P> FreelistPage<P> {
	const NEXT_PAGE_ID_OFFSET: usize = PAGE_HEADER_SIZE;
	const LENGTH_OFFSET: usize = Self::NEXT_PAGE_ID_OFFSET + size_of::<PageIdRepr>();
	const ITEMS_OFFSET: usize = Self::LENGTH_OFFSET + size_of::<u16>();

	pub const NUM_SLOTS: usize = (PAGE_BODY_SIZE - Self::ITEMS_OFFSET) / size_of::<PageIdRepr>();

	pub fn new_unchecked(page: P) -> Self {
		Self(page)
	}

	fn offset_for_index(index: usize) -> Option<usize> {
		let offset = Self::ITEMS_OFFSET + index * mem::size_of::<PageIdRepr>();
		if offset >= PAGE_BODY_SIZE {
			return None;
		}
		Some(offset)
	}
}

impl<P: ReadPage> FreelistPage<P> {
	pub fn new(page: P) -> Result<Self, DatabaseError> {
		assert_page_kind(&page, PageKind::FreelistBlock)?;
		Ok(Self::new_unchecked(page))
	}

	pub fn get_next_page_id(&self) -> Result<Option<PageId>, DatabaseError> {
		let mut repr = PageIdRepr::new_zeroed();
		self.0
			.read(Self::NEXT_PAGE_ID_OFFSET, repr.as_bytes_mut())?;
		Ok(repr.into())
	}

	pub fn get_length(&self) -> Result<usize, DatabaseError> {
		let mut repr = [0; 2];
		self.0.read(Self::LENGTH_OFFSET, &mut repr)?;
		Ok(u16::from_ne_bytes(repr).into())
	}

	pub fn is_full(&self) -> Result<bool, DatabaseError> {
		Ok(self.get_length()? >= Self::NUM_SLOTS)
	}

	pub fn get_item(&self, index: usize) -> Result<Option<PageId>, DatabaseError> {
		let mut repr = PageIdRepr::new_zeroed();
		let Some(offset) = Self::offset_for_index(index) else {
			return Ok(None);
		};
		self.0.read(offset, repr.as_bytes_mut())?;
		Ok(repr.into())
	}
}

impl<P: WritePage> FreelistPage<P> {
	pub fn init(&mut self) -> Result<(), DatabaseError> {
		set_page_kind(&mut self.0, PageKind::FreelistBlock)?;
		self.set_next_page_id(None)?;
		self.set_length(0)?;
		Ok(())
	}

	pub fn set_next_page_id(&mut self, value: Option<PageId>) -> Result<(), DatabaseError> {
		let repr = PageIdRepr::from(value);
		self.0.write(Self::NEXT_PAGE_ID_OFFSET, repr.as_bytes())?;
		Ok(())
	}

	fn set_length(&mut self, value: usize) -> Result<(), DatabaseError> {
		let repr = u16::try_from(value).expect("Freelist page length must be 16-bit!");
		self.0.write(Self::LENGTH_OFFSET, &repr.to_ne_bytes())?;
		Ok(())
	}

	fn set_item(&mut self, index: usize, value: Option<PageId>) -> Result<(), DatabaseError> {
		let repr = PageIdRepr::from(value);
		let Some(offset) = Self::offset_for_index(index) else {
			return Err(DatabaseError::PageIndexOutOfBounds);
		};
		self.0.write(offset, repr.as_bytes())?;
		Ok(())
	}
}

impl<P: ReadPage + WritePage> FreelistPage<P> {
	pub fn push_item(&mut self, value: PageId) -> Result<(), DatabaseError> {
		let index = self.get_length()?;
		self.set_item(index, Some(value))?;
		self.set_length(index + 1)?;
		Ok(())
	}

	pub fn pop_item(&mut self) -> Result<Option<PageId>, DatabaseError> {
		let mut index = self.get_length()?;
		loop {
			if index == 0 {
				return Ok(None);
			}
			index -= 1;
			if let Some(item) = self.get_item(index)? {
				self.set_length(index)?;
				return Ok(Some(item));
			}
		}
	}
}

pub(super) struct BlockPage<P>(P);

impl<P> BlockPage<P> {
	const LEAF_BLOCK_SIZE: usize = 32 * B;
	const DEGREE_MASK: usize = (Self::LEAF_BLOCK_SIZE + 1).next_power_of_two() - 1;
	const ALLOC_TREE_SIZE: usize =
		Self::get_alloc_tree_size(PAGE_BODY_SIZE - PAGE_HEADER_SIZE, Self::LEAF_BLOCK_SIZE);

	const ALLOC_TREE_OFFSET: usize = PAGE_HEADER_SIZE;
	const BODY_OFFSET: usize = Self::ALLOC_TREE_OFFSET + Self::ALLOC_TREE_SIZE;

	const BODY_SIZE: usize = PAGE_BODY_SIZE - Self::BODY_OFFSET;
	const NUM_LEAF_NODES: usize = Self::BODY_SIZE / Self::LEAF_BLOCK_SIZE;
	const ROOT_BLOCK_SIZE: usize = Self::NUM_LEAF_NODES * Self::LEAF_BLOCK_SIZE;
	const MAX_DEGREE: usize = Self::NUM_LEAF_NODES.next_power_of_two().ilog2() as usize;

	const fn get_alloc_tree_size(body_size: usize, leaf_block_size: usize) -> usize {
		let preemptive_num_leaves = body_size / leaf_block_size;
		let preemptive_num_nodes =
			preemptive_num_leaves.next_power_of_two() + preemptive_num_leaves - 1;

		(preemptive_num_nodes + 7) / 8
	}

	pub fn new_unchecked(page: P) -> Self {
		Self(page)
	}

	fn get_block_pos(index: u16) -> (usize, usize) {
		let block_degree = usize::from(index) & Self::DEGREE_MASK;
		let block_pos = usize::from(index) / (Self::LEAF_BLOCK_SIZE << block_degree);
		(block_pos, block_degree)
	}
}

impl<P: ReadPage> BlockPage<P> {
	fn get_alloc_tree_value(degree: usize, pos: usize) {
		todo!()
	}

	fn find_free_block(size: usize) {
		todo!()
	}
}
