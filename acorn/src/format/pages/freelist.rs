use std::{mem::size_of, num::NonZeroU32};

use crate::format::Error;

use super::Page;

#[repr(C)]
pub struct FreelistPage {
	pub next: Option<NonZeroU32>,
	pub length: u32,
	pub pages: [Option<NonZeroU32>],
}

impl Page for FreelistPage {
	const HEADER_SIZE: usize = size_of::<Option<NonZeroU32>>() + size_of::<u32>();
	const ITEM_SIZE: usize = size_of::<Option<NonZeroU32>>();
}

impl FreelistPage {
	#[inline]
	pub fn is_full(&self) -> bool {
		self.length as usize == self.pages.len()
	}

	#[inline]
	pub fn is_empty(&self) -> bool {
		self.length == 0
	}

	pub fn push_page(&mut self, page_number: NonZeroU32) -> bool {
		if self.is_full() {
			return false;
		}

		self.pages[self.length as usize] = Some(page_number);
		self.length += 1;
		true
	}

	pub fn pop_page(&mut self) -> Result<Option<NonZeroU32>, Error> {
		if self.is_empty() {
			return Ok(None);
		}

		let Some(page) = self.pages[self.length as usize - 1] else {
			return Err(Error::Corrupted);
		};
		self.length -= 1;
		Ok(Some(page))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn read_freelist_page() {
		let mut bytes = Vec::new();
		bytes.extend(0_u32.to_ne_bytes());
		bytes.extend(3_u32.to_ne_bytes());
		bytes.extend(1_u32.to_ne_bytes());
		bytes.extend(2_u32.to_ne_bytes());
		bytes.extend(0_u32.to_ne_bytes());
		bytes.extend([0x00, 0x00]);

		let page = FreelistPage::new(&bytes);

		assert_eq!(page.next, None);
		assert_eq!(page.length, 3);
		assert_eq!(page.pages, [NonZeroU32::new(1), NonZeroU32::new(2), None])
	}

	#[test]
	fn pages_push_format() {
		let mut bytes = Vec::new();
		bytes.extend(0_u32.to_ne_bytes());
		bytes.extend(0_u32.to_ne_bytes());
		bytes.extend(0_u32.to_ne_bytes());

		let page = FreelistPage::new_mut(&mut bytes);

		assert!(page.push_page(NonZeroU32::new(69).unwrap()));

		let mut expected = Vec::new();
		expected.extend(0_u32.to_ne_bytes());
		expected.extend(1_u32.to_ne_bytes());
		expected.extend(69_u32.to_ne_bytes());

		assert_eq!(bytes, expected);
	}

	#[test]
	fn pages_push_and_pop() {
		let mut bytes = Vec::new();
		bytes.extend(0_u32.to_ne_bytes());
		bytes.extend(0_u32.to_ne_bytes());
		bytes.extend(0_u32.to_ne_bytes());

		let page = FreelistPage::new_mut(&mut bytes);

		assert!(page.push_page(NonZeroU32::new(69).unwrap()));
		assert!(!page.push_page(NonZeroU32::new(420).unwrap()));
		assert_eq!(page.pop_page().unwrap(), NonZeroU32::new(69));
		assert_eq!(page.pop_page().unwrap(), None);
	}
}
