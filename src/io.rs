use std::io::{ErrorKind, IoSlice, IoSliceMut, Write};
use bytes::buf::BufMut;
use tokio::sync::mpsc::error::TryRecvError;
use crate::Message;
use crate::stream::BidiStream;

/// The `TryRead` trait allows reading bytes from a source.
/// In this case the source is a quic stream.
pub trait TryRead {
    
    /// Tries to read data from the stream into the provided buffer, returning how
    /// many bytes were read.
    ///
    /// Receives any pending data from the socket but does not wait for new data
    /// to arrive. On success, returns the number of bytes read. Because
    /// `try_read()` is non-blocking, the buffer does not have to be stored by
    /// the async task and can exist entirely on the stack.
    ///
    /// # Return
    ///
    /// If data is successfully read, `Ok(n)` is returned, where `n` is the
    /// number of bytes read.
    ///
    /// If the stream is not ready to read data, or is already closed an error 
    /// will be returned.
    fn try_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;

    /// Tries to read data from the stream into the provided buffer, advancing the
    /// buffer's internal cursor, returning how many bytes were read.
    ///
    /// Receives any pending data from the stream but does not wait for new data
    /// to arrive. On success, returns the number of bytes read. Because
    /// `try_read_buf()` is non-blocking, the buffer does not have to be stored by
    /// the async task and can exist entirely on the stack.
    ///
    /// # Return
    ///
    /// If data is successfully read, `Ok(n)` is returned, where `n` is the
    /// number of bytes read.
    ///
    /// If the stream is not ready to read data, or is already closed an error 
    /// will be returned.
    fn try_read_buf<B: BufMut>(&mut self, buf: &mut B) -> std::io::Result<usize>;

    /// Tries to read data from the stream into the provided buffers, returning
    /// how many bytes were read.
    ///
    /// Data is copied to fill each buffer in order, with the final buffer
    /// written to possibly being only partially filled. This method behaves
    /// equivalently to a single call to [`try_read()`] with concatenated
    /// buffers.
    ///
    /// Receives any pending data from the socket but does not wait for new data
    /// to arrive. On success, returns the number of bytes read. Because
    /// `try_read_vectored()` is non-blocking, the buffer does not have to be
    /// stored by the async task and can exist entirely on the stack.
    ///
    /// [`try_read()`]: TryRead::try_read()
    ///
    /// # Return
    ///
    /// If data is successfully read, `Ok(n)` is returned, where `n` is the
    /// number of bytes read.
    ///
    /// If the stream is not ready to read data, or is already closed an error 
    /// will be returned.
    fn try_read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> std::io::Result<usize>;
}

/// The `TryWrite` trait allows writing bytes to a destination.
/// In this case the destination is a quic stream.
pub trait TryWrite {
    
    /// Try to write a buffer to the stream, returning how many bytes were
    /// written.
    ///
    /// The function will attempt to write the entire contents of `buf`, but
    /// only part of the buffer may be written.
    ///
    /// # Return
    ///
    /// If data is successfully written, `Ok(n)` is returned, where `n` is the
    /// number of bytes written.
    /// 
    /// If the stream is not ready to write data, or is already closed an error 
    /// will be returned.
    fn try_write(&mut self, buf: &[u8]) -> std::io::Result<usize>;

    /// Tries to write several buffers to the stream, returning how many bytes
    /// were written.
    ///
    /// Data is written from each buffer in order, with the final buffer read
    /// from possible being only partially consumed. This method behaves
    /// equivalently to a single call to [`try_write()`] with concatenated
    /// buffers.
    ///
    /// [`try_write()`]: TryWrite::try_write()
    ///
    /// # Return
    ///
    /// If data is successfully written, `Ok(n)` is returned, where `n` is the
    /// number of bytes written.
    ///
    /// If the stream is not ready to write data, or is already closed an error 
    /// will be returned.
    fn try_write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> std::io::Result<usize>;
}

impl TryRead for BidiStream {
    fn try_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut vec = Vec::with_capacity(buf.len());
        self.try_read_buf(&mut vec)?;
        buf.as_mut().write(vec.as_slice())
    }

    fn try_read_buf<B: BufMut>(&mut self, buf: &mut B) -> std::io::Result<usize> {
        let mut total_read = buf.remaining_mut().min(self.buffer_read.len());
        buf.put_slice(&self.buffer_read[..buf.remaining_mut().min(self.buffer_read.len())]);
        let remaining = self.buffer_read.len() - total_read;
        self.buffer_read.truncate(remaining);
        loop {
            match self.rx.try_recv() {
                Ok(message) => match message {
                    Ok(message) => {
                        if let Message::Data { stream_id: _, bytes, fin } = message {
                            if fin {
                                self.rx.close();
                            }
                            self.buffer_read.extend_from_slice(&bytes);
                        }
                    }
                    Err(err) => {
                        Err(std::io::Error::new(ErrorKind::Other, err.to_string()))?
                    }
                }
                Err(TryRecvError::Empty) => {
                    break
                }
                Err(err) => {
                    Err(std::io::Error::new(ErrorKind::Other, err.to_string()))?
                }
            }
        }
        let to_write = buf.remaining_mut().min(self.buffer_read.len());
        buf.put_slice(&self.buffer_read[..to_write]);
        let remaining = self.buffer_read.len() - total_read;
        self.buffer_read.truncate(remaining);
        total_read += to_write;
        Ok(total_read)
    }

    fn try_read_vectored(&mut self, bufs: &mut [IoSliceMut<'_>]) -> std::io::Result<usize> {
        let mut total_read = 0;
        for buf in bufs {
            total_read += self.try_read(buf)?;
        }
        Ok(total_read)
    }
}

impl TryWrite for BidiStream {
    fn try_write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.tx.send(Message::Data {
            stream_id: self.id,
            bytes: buf.to_vec(),
            fin: false,
        }) {
            Ok(()) => Ok(buf.len()),
            Err(err) => Err(std::io::Error::new(ErrorKind::Other, err.to_string()))?
        }
    }

    fn try_write_vectored(&mut self, bufs: &[IoSlice<'_>]) -> std::io::Result<usize> {
        let mut total_written = 0;
        for buf in bufs {
            total_written += self.try_write(buf)?;
        }
        Ok(total_written)
    }
}