use std::{
	fs::{File, OpenOptions},
	io::{self, BufReader, Read, Seek, SeekFrom, Write},
	mem::size_of,
	path::Path,
};

use byte_view::{ByteView, ViewBuf};
use thiserror::Error;

use crate::{
	consts::WAL_MAGIC,
	index::PageId,
	utils::{byte_order::ByteOrder, units::display_size},
};

#[derive(Debug, Error)]
pub enum InitError {
	#[error(transparent)]
	Io(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum LoadError {
	#[error("This file is not an acorn WAL file")]
	NotAWalFile,

	#[error("The WAL file is corrupted")]
	Corrupted,

	#[error(
		"Page size mismatch; should be {}, but found {}",
		display_size(*_1 as usize),
		display_size(*_0 as usize)
	)]
	PageSizeMismatch(u16, u16),

	#[error("Byte order mismatch; should be {}, but found {0}", ByteOrder::NATIVE)]
	ByteOrderMismatch(ByteOrder),

	#[error(transparent)]
	Io(#[from] io::Error),
}

pub struct InitParams {
	pub page_size: u16,
}

pub struct LoadParams {
	pub page_size: u16,
}

pub struct Wal<T: Seek + Read + Write> {
	log_start: u64,
	page_size: u16,
	buf: Vec<u8>,
	file: T,
}

impl Wal<File> {
	pub fn init_file(path: impl AsRef<Path>, params: InitParams) -> Result<(), InitError> {
		let mut file = OpenOptions::new()
			.write(true)
			.truncate(true)
			.create(true)
			.open(path)?;
		Self::init(&mut file, params)
	}

	pub fn load_file(path: impl AsRef<Path>, params: LoadParams) -> Result<Self, LoadError> {
		let file = OpenOptions::new().read(true).append(true).open(path)?;
		Self::load(file, params)
	}

	pub fn clear(&mut self) -> Result<(), io::Error> {
		self.file.set_len(self.log_start)?;
		Ok(())
	}
}

impl<T: Seek + Read + Write> Wal<T> {
	pub fn init(file: &mut T, params: InitParams) -> Result<(), InitError> {
		let mut header: ViewBuf<WalHeader> = ViewBuf::new();
		*header = WalHeader {
			magic: WAL_MAGIC,
			log_start: size_of::<WalHeader>() as u16,
			page_size: params.page_size,
			byte_order: ByteOrder::NATIVE as u8,
		};

		file.seek(SeekFrom::Start(0))?;
		file.write_all(header.as_bytes())?;
		Ok(())
	}

	pub fn load(mut file: T, params: LoadParams) -> Result<Self, LoadError> {
		let mut header: ViewBuf<WalHeader> = ViewBuf::new();
		file.seek(SeekFrom::Start(0))?;
		file.read_exact(header.as_bytes_mut())?;

		if header.magic != WAL_MAGIC {
			return Err(LoadError::NotAWalFile);
		}
		let Some(byte_order) = ByteOrder::from_byte(header.byte_order) else {
			return Err(LoadError::Corrupted);
		};
		if byte_order != ByteOrder::NATIVE {
			return Err(LoadError::ByteOrderMismatch(byte_order));
		}
		if header.page_size != params.page_size {
			return Err(LoadError::PageSizeMismatch(
				header.page_size,
				params.page_size,
			));
		}

		file.seek(SeekFrom::Start(header.log_start as u64))?;
		Ok(Self {
			file,
			buf: Vec::new(),
			log_start: header.log_start as u64,
			page_size: header.page_size,
		})
	}

	pub fn log_write(&mut self, page_id: PageId, data: &[u8]) {
		self.buf.extend(ViewBuf::from(page_id).as_bytes());
		self.buf.extend(data);
	}

	pub fn commit(&mut self) -> Result<(), io::Error> {
		self.file.write_all(&self.buf)?;
		self.buf.clear();
		Ok(())
	}

	pub fn iter(&mut self) -> Result<Iter<T>, io::Error> {
		self.file.seek(SeekFrom::Start(self.log_start))?;
		Ok(Iter {
			page_size: self.page_size,
			file: BufReader::new(&mut self.file),
		})
	}

	pub fn cancel(&mut self) {
		self.buf.clear();
	}
}

#[derive(ByteView, Debug, PartialEq, Eq)]
struct WalHeader {
	magic: [u8; 4],
	log_start: u16,
	page_size: u16,
	byte_order: u8,
}

pub struct Iter<'a, T: Read> {
	page_size: u16,
	file: BufReader<&'a mut T>,
}

impl<'a, T: Read> Iterator for Iter<'a, T> {
	type Item = Result<(PageId, Vec<u8>), io::Error>;

	fn next(&mut self) -> Option<Self::Item> {
		let mut page_id_buf: ViewBuf<PageId> = ViewBuf::new();
		let mut page_buf: Vec<u8> = vec![0; self.page_size as usize];

		let bytes_read = match self.file.read(page_id_buf.as_bytes_mut()) {
			Ok(bytes_read) => bytes_read,
			Err(err) => return Some(Err(err)),
		};
		if bytes_read != page_id_buf.size() {
			return None;
		}

		match self.file.read_exact(&mut page_buf) {
			Ok(bytes_read) => bytes_read,
			Err(err) => return Some(Err(err)),
		};

		Some(Ok((*page_id_buf, page_buf)))
	}
}

#[cfg(test)]
mod tests {
	use std::{alloc::Layout, io::Cursor};

	use crate::utils::aligned_buf::AlignedBuffer;

	use super::*;

	#[test]
	fn init_wal_file() {
		let mut buf = AlignedBuffer::with_layout(Layout::new::<WalHeader>());
		let mut file = Cursor::new(buf.as_mut());

		Wal::init(&mut file, InitParams { page_size: 1024 }).unwrap();

		let mut expected: ViewBuf<WalHeader> = ViewBuf::new();
		*expected = WalHeader {
			magic: *b"ACNL",
			log_start: 10,
			byte_order: ByteOrder::NATIVE as u8,
			page_size: 1024,
		};
		assert_eq!(WalHeader::from_bytes(&buf), &*expected);
	}

	#[test]
	fn load_wal_file() {
		let mut file = Cursor::new(Vec::<u8>::new());
		Wal::init(&mut file, InitParams { page_size: 1024 }).unwrap();

		Wal::load(file, LoadParams { page_size: 1024 }).unwrap();
	}

	#[test]
	fn log_writes() {
		let mut data: Vec<u8> = Vec::new();
		let mut file = Cursor::new(&mut data);
		Wal::init(&mut file, InitParams { page_size: 8 }).unwrap();

		let mut wal = Wal::load(file, LoadParams { page_size: 8 }).unwrap();
		wal.log_write(PageId::new(0, 10), &[10; 8]);
		wal.log_write(PageId::new(0, 12), &[15; 8]);
		wal.commit().unwrap();

		assert_eq!(
			&data[size_of::<WalHeader>()..],
			&[
				ViewBuf::from(PageId::new(0, 10)).as_bytes(),
				&[10; 8],
				ViewBuf::from(PageId::new(0, 12)).as_bytes(),
				&[15; 8]
			]
			.concat()
		);
	}

	#[test]
	fn dont_log_uncommitted_writes() {
		let mut data: Vec<u8> = Vec::new();
		let mut file = Cursor::new(&mut data);
		Wal::init(&mut file, InitParams { page_size: 8 }).unwrap();

		let mut wal = Wal::load(file, LoadParams { page_size: 8 }).unwrap();
		wal.log_write(PageId::new(0, 10), &[10; 8]);
		wal.log_write(PageId::new(0, 12), &[15; 8]);

		assert!(data[size_of::<WalHeader>()..].is_empty());
	}

	#[test]
	fn iter_logs() {
		let mut data: Vec<u8> = Vec::new();
		let mut file = Cursor::new(&mut data);
		Wal::init(&mut file, InitParams { page_size: 8 }).unwrap();

		let mut wal = Wal::load(file, LoadParams { page_size: 8 }).unwrap();
		wal.log_write(PageId::new(0, 10), &[10; 8]);
		wal.log_write(PageId::new(0, 12), &[15; 8]);
		wal.commit().unwrap();

		wal.log_write(PageId::new(0, 5), &[25; 8]);
		wal.commit().unwrap();

		let mut iter = wal.iter().unwrap();
		assert_eq!(
			iter.next().unwrap().unwrap(),
			(PageId::new(0, 10), vec![10; 8])
		);
		assert_eq!(
			iter.next().unwrap().unwrap(),
			(PageId::new(0, 12), vec![15; 8])
		);
		assert_eq!(
			iter.next().unwrap().unwrap(),
			(PageId::new(0, 5), vec![25; 8])
		);
		assert!(iter.next().is_none());
	}
}
