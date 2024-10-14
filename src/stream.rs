use std::marker::PhantomData;
use std::{io, task::Poll};
use bytes::BytesMut;
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
    pub(crate) buffer_read: BytesMut,
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
            buffer_read: BytesMut::with_capacity(u16::MAX as usize),
        }
    }
}

pub struct UniStream<M: UniMode> {
    pub(crate) id: u64,
    pub(crate) rx: UnboundedReceiver<Result<Message>>,
    pub(crate) tx: UnboundedSender<Message>,
    pub(crate) buffer: BytesMut,
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
            buffer: BytesMut::with_capacity(u16::MAX as usize),
            _ty: Default::default(),
        }
    }
}