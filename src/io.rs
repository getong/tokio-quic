use std::io::{ErrorKind, IoSlice, IoSliceMut, Write};
use bytes::buf::BufMut;
use bytes::BytesMut;
use tokio::sync::mpsc::error::TryRecvError;
use crate::Message;
use crate::stream::BidiStream;

/// The `TryRead` trait allows reading bytes from a source.
/// In this case the source is a quic stream.
pub trait TryRead {
    fn try_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
    fn try_read_buf<B: BufMut>(&mut self, buf: &mut B) -> std::io::Result<usize>;
}

///
pub trait TryWrite {
    fn try_write(&mut self, buf: &[u8]) -> std::io::Result<usize>;
    fn try_write_vectored(&mut self, buf: &[IoSlice<'_>]) -> std::io::Result<usize>;
}

impl TryRead for BidiStream {
    fn try_read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut vec = Vec::new();
        self.try_read_buf(&mut vec)?;
        buf.as_mut().write(vec.as_slice())
    }

    fn try_read_buf<B: BufMut>(&mut self, buf: &mut B) -> std::io::Result<usize> {
        match self.rx.try_recv() {
            Ok(message) => match message {
                Ok(message) => {
                    match message {
                        Message::Data { stream_id: _, bytes, fin: _ } => {
                            buf.put_slice(&bytes);
                            Ok(bytes.len())
                        }
                        Message::Close(id) => {
                            Err(std::io::Error::new(ErrorKind::BrokenPipe, format!("Stream {id} is closed!")))
                        }
                    }
                }
                Err(err) => {
                    Err(std::io::Error::new(ErrorKind::Other, err.to_string()))
                }
            }
            Err(TryRecvError::Empty) => {
                Ok(0)
            }
            Err(err) => {
                Err(std::io::Error::new(ErrorKind::Other, err.to_string()))
            }
        }
    }
}

impl TryWrite for BidiStream {
    fn try_write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        todo!()
    }

    fn try_write_vectored(&mut self, buf: &[IoSlice<'_>]) -> std::io::Result<usize> {
        todo!()
    }
}