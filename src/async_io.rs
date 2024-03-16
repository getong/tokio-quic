use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use crate::Message;
use crate::stream::{BidiStream, Readable, UniStream, Writeable};

impl AsyncRead for BidiStream {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(message)) => match message {
                Ok(Message::Data {
                       stream_id: _,
                       bytes,
                       fin,
                   }) => {
                    if fin {
                        self.rx.close();
                    }
                    buf.put_slice(bytes.as_slice());
                    buf.set_filled(bytes.len());
                    Poll::Ready(Ok(()))
                }
                Ok(Message::Close(_id)) => {
                    self.rx.close();
                    Poll::Ready(Ok(()))
                },
                Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, err.to_string()))),
            },
            Poll::Ready(None) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "No new data is available to be read, stream is closed!",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for BidiStream {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let message = Message::Data {
            stream_id: self.id,
            bytes: buf.to_vec(),
            fin: false,
        };
        match self.tx.send(message) {
            Ok(_) => Poll::Ready(Ok(buf.len())),
            Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, err))),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let message = Message::Close(self.id);
        match self.tx.send(message) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, err))),
        }
    }
}

impl AsyncRead for UniStream<Readable> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(message)) => match message {
                Ok(Message::Data {
                       stream_id: _,
                       bytes,
                       fin,
                   }) => {
                    buf.put_slice(bytes.as_slice());
                    buf.set_filled(bytes.len());
                    if fin {
                        self.rx.close();
                    }
                    Poll::Ready(Ok(()))
                }
                Ok(Message::Close(_id)) => {
                    self.rx.close();
                    Poll::Ready(Ok(()))
                },
                Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, err.to_string()))),
            },
            Poll::Ready(None) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "No new data is available to be read, stream is closed!",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for UniStream<Writeable> {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        let message = Message::Data {
            stream_id: self.id,
            bytes: buf.to_vec(),
            fin: false,
        };
        match self.tx.send(message) {
            Ok(_) => Poll::Ready(Ok(buf.len())),
            Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, err))),
        }
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        let message = Message::Close(self.id);
        match self.tx.send(message) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, err))),
        }
    }
}
