use core::fmt;
use std::mem::size_of;

use byte_view::ByteView;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, ByteView)]
pub struct PageId {
	pub segment_num: u32,
	pub page_num: u16,
}

impl PageId {
	#[inline]
	pub fn new(segment_num: u32, page_num: u16) -> Self {
		Self {
			segment_num,
			page_num,
		}
	}

	pub fn as_bytes(&self) -> [u8; size_of::<Self>()] {
		let mut bytes = [0; 8];
		bytes[0..4].copy_from_slice(&self.segment_num.to_ne_bytes());
		bytes[4..6].copy_from_slice(&self.page_num.to_ne_bytes());
		bytes
	}
}

impl fmt::Display for PageId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{:08x}:{:04x}", self.segment_num, self.page_num)
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct StorageIndex {
	pub segment_num: u32,
	pub page_num: u16,
	pub index: u16,
}

impl StorageIndex {
	#[inline]
	pub fn new(page_id: PageId, index: u16) -> Self {
		Self {
			segment_num: page_id.segment_num,
			page_num: page_id.page_num,
			index,
		}
	}

	#[inline]
	pub fn page_id(self) -> PageId {
		PageId::new(self.segment_num, self.page_num)
	}
}

impl fmt::Display for StorageIndex {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		write!(f, "{}:{:04x}", self.page_id(), self.index)
	}
}
