//! IQ file reader for SDR sample files.
//!
//! Supports common formats:
//! - `i8`: int8 interleaved I/Q (RTL-SDR, HackRF default)
//! - `i16`: int16 interleaved I/Q (GNSS-SDR test dataset, USRP)

use num_complex::Complex;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

/// Supported IQ sample formats.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IqFormat {
    /// int8 I/Q interleaved: I0, Q0, I1, Q1, ...
    Int8,
    /// int16 I/Q interleaved (little-endian)
    Int16,
}

/// IQ file reader.
pub struct IqReader {
    reader: BufReader<File>,
    format: IqFormat,
    raw_buffer: Vec<u8>,
    /// Total file size in bytes
    file_size: u64,
    /// Bytes consumed so far
    bytes_read: u64,
}

impl IqReader {
    /// Open an IQ file.
    pub fn open(path: impl AsRef<Path>, format: IqFormat) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let file_size = file.metadata()?.len();
        Ok(Self {
            reader: BufReader::with_capacity(256 * 1024, file),
            format,
            raw_buffer: Vec::new(),
            file_size,
            bytes_read: 0,
        })
    }

    /// Number of complex samples in the file.
    pub fn total_samples(&self) -> u64 {
        let bytes_per_sample = match self.format {
            IqFormat::Int8 => 2,
            IqFormat::Int16 => 4,
        };
        self.file_size / bytes_per_sample
    }

    /// Read up to `count` complex samples into the provided buffer.
    /// Returns the actual number of samples read.
    pub fn read(&mut self, buffer: &mut [Complex<f64>], count: usize) -> std::io::Result<usize> {
        let n = count.min(buffer.len());
        match self.format {
            IqFormat::Int8 => self.read_int8(buffer, n),
            IqFormat::Int16 => self.read_int16(buffer, n),
        }
    }

    /// Seek to a specific sample index.
    pub fn seek_to_sample(&mut self, sample_index: u64) -> std::io::Result<()> {
        let bytes_per_sample = match self.format {
            IqFormat::Int8 => 2,
            IqFormat::Int16 => 4,
        };
        let byte_offset = sample_index * bytes_per_sample;
        use std::io::Seek;
        self.reader.seek(std::io::SeekFrom::Start(byte_offset))?;
        self.bytes_read = byte_offset;
        Ok(())
    }

    fn read_int8(&mut self, buffer: &mut [Complex<f64>], n: usize) -> std::io::Result<usize> {
        let bytes = self.read_raw(n * 2)?;
        let samples_read = bytes / 2;
        for i in 0..samples_read {
            let i_val = self.raw_buffer[i * 2] as i8;
            let q_val = self.raw_buffer[i * 2 + 1] as i8;
            buffer[i] = Complex::new(i_val as f64, q_val as f64);
        }
        self.bytes_read += bytes as u64;
        Ok(samples_read)
    }

    fn read_int16(&mut self, buffer: &mut [Complex<f64>], n: usize) -> std::io::Result<usize> {
        let bytes = self.read_raw(n * 4)?;
        let samples_read = bytes / 4;
        for i in 0..samples_read {
            let i_val = i16::from_le_bytes([self.raw_buffer[i * 4], self.raw_buffer[i * 4 + 1]]);
            let q_val =
                i16::from_le_bytes([self.raw_buffer[i * 4 + 2], self.raw_buffer[i * 4 + 3]]);
            buffer[i] = Complex::new(i_val as f64, q_val as f64);
        }
        self.bytes_read += bytes as u64;
        Ok(samples_read)
    }

    fn read_raw(&mut self, byte_count: usize) -> std::io::Result<usize> {
        self.raw_buffer.resize(byte_count, 0);
        let mut bytes_read = 0;
        while bytes_read < byte_count {
            let read = self
                .reader
                .read(&mut self.raw_buffer[bytes_read..byte_count])?;
            if read == 0 {
                break;
            }
            bytes_read += read;
        }
        Ok(bytes_read)
    }
}
