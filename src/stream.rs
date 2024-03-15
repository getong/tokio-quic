use std::marker::PhantomData;
use std::{io, task::Poll};

use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::mpsc::{UnboundedReceiver, UnboundedSender},
};

use crate::{error::Result, Message};

pub trait UniMode {}
pub struct Writeable;
impl UniMode for Writeable {}
pub struct Readable;
impl UniMode for Readable {}

/// Used for type hints
pub(crate) trait QuicStream {
    fn id(&self) -> u64;
}

#[derive(Debug)]
///// Implements the `AsyncRead` and `AsyncWrite` traits.
/////
///// Shutdown permanently closes the quic stream.
pub struct UncheckedQuicStream {
    pub(crate) id: u64,
    #[allow(dead_code)]
    pub(crate) rx: UnboundedReceiver<Result<Message>>,
    #[allow(dead_code)]
    pub(crate) tx: UnboundedSender<Message>,
}

impl QuicStream for UncheckedQuicStream {
    fn id(&self) -> u64 {
        self.id
    }
}

pub struct BidiStream {
    pub(crate) id: u64,
    pub(crate) rx: UnboundedReceiver<Result<Message>>,
    pub(crate) tx: UnboundedSender<Message>,
}

impl QuicStream for BidiStream {
    fn id(&self) -> u64 {
        self.id
    }
}

impl From<UncheckedQuicStream> for BidiStream {
    fn from(stream: UncheckedQuicStream) -> Self {
        Self {
            id: stream.id,
            rx: stream.rx,
            tx: stream.tx,
        }
    }
}

pub struct UniStream<M: UniMode> {
    pub(crate) id: u64,
    pub(crate) rx: UnboundedReceiver<Result<Message>>,
    pub(crate) tx: UnboundedSender<Message>,
    _ty: PhantomData<M>,
}

impl<M: UniMode> QuicStream for UniStream<M> {
    fn id(&self) -> u64 {
        self.id
    }
}

impl<M: UniMode> UniStream<M> {
    pub(crate) fn new(
        id: u64,
        rx: UnboundedReceiver<Result<Message>>,
        tx: UnboundedSender<Message>,
    ) -> Self {
        Self {
            id,
            rx,
            tx,
            _ty: Default::default(),
        }
    }
}

impl AsyncRead for BidiStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(message)) => match message {
                Ok(Message::Data {
                    stream_id: _,
                    bytes,
                    fin: _,
                }) => {
                    buf.put_slice(bytes.as_slice());
                    buf.set_filled(bytes.len());
                    Poll::Ready(Ok(()))
                }
                Ok(Message::Close(_id)) => Poll::Ready(Ok(())),
                Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, err.to_string()))),
            },
            Poll::Ready(None) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "No new data is available to be read!",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for BidiStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, io::Error>> {
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
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), io::Error>> {
        let message = Message::Close(self.id);
        match self.tx.send(message) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, err))),
        }
    }
}

impl AsyncRead for UniStream<Readable> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.rx.poll_recv(cx) {
            Poll::Ready(Some(message)) => match message {
                Ok(Message::Data {
                    stream_id: _,
                    bytes,
                    fin: _,
                }) => {
                    buf.put_slice(bytes.as_slice());
                    buf.set_filled(bytes.len());
                    Poll::Ready(Ok(()))
                }
                Ok(Message::Close(_id)) => Poll::Ready(Ok(())),
                Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::Other, err.to_string()))),
            },
            Poll::Ready(None) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "No new data is available to be read!",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl AsyncWrite for UniStream<Writeable> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, io::Error>> {
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
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> Poll<std::result::Result<(), io::Error>> {
        let message = Message::Close(self.id);
        match self.tx.send(message) {
            Ok(_) => Poll::Ready(Ok(())),
            Err(err) => Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, err))),
        }
    }
}
