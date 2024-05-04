use std::num::NonZeroU16;

use thiserror::Error;

use crate::files::FileError;

mod physical;

#[derive(Debug, Error)]
pub(crate) enum StorageError {
	#[error(transparent)]
	File(#[from] FileError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageId {
	pub segment_num: u32,
	pub page_num: NonZeroU16,
}

impl PageId {
	fn new(segment_num: u32, page_num: NonZeroU16) -> Self {
		Self {
			segment_num,
			page_num,
		}
	}
}
