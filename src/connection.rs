use log::trace;
use std::{collections::HashMap, marker::PhantomData, sync::Arc};
use bytes::BytesMut;
use tokio::{
    sync::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        Mutex,
    },
    task::JoinHandle,
};

use crate::backend::Driver;
use crate::stream::{BidiStream, Readable, UniStream, Writeable};
use crate::{
    backend::{client, server},
    error::Result,
    stream::UncheckedQuicStream,
    Message,
};

pub trait Backend {}

/// Indicates that the connection is from the client to a server.
pub struct ToServer;

impl Backend for ToServer {}

/// Indicates that the connection is from the server to a client.
pub struct ToClient;

impl Backend for ToClient {}

/// Sum of incoming stream types
pub enum Incoming {
    Bidi(BidiStream),
    Uni(UniStream<Readable>),
}

impl Incoming {
    pub(crate) fn from_unchecked(
        stream: Option<UncheckedQuicStream>,
        is_server: bool,
    ) -> Option<Self> {
        if let Some(stream) = stream {
            Some(match (stream.id & 0b11, is_server) {
                (0b10, true) | (0b11, false) => {
                    Self::Uni(UniStream::new(stream.id, stream.rx, stream.tx))
                }
                (_, _) => Self::Bidi(BidiStream {
                    id: stream.id,
                    rx: stream.rx,
                    tx: stream.tx,
                    buffer_read: BytesMut::with_capacity(u16::MAX as usize),
                }),
            })
        } else {
            None
        }
    }
}

type AsyncStreamMap = Arc<Mutex<HashMap<u64, UnboundedSender<Result<Message>>>>>;

/// A `QuicConnection` represents a connection to a remote host.
///
/// ```rs
/// connection.open().await;
/// ```
/// Is used to open a new bidi stream.
///
/// ```rs
/// connection.incoming().await.unwrap();
/// ```
/// Waits for an incoming stream from remote.
pub struct QuicConnection<T: Backend + Send> {
    #[allow(unused)]
    handle: JoinHandle<Result<()>>,
    stream_map: AsyncStreamMap,
    // Map each stream to a `Sender`
    message_send: UnboundedSender<Message>,
    // This is passed to each stream.
    incoming_recv: UnboundedReceiver<UncheckedQuicStream>,
    state: PhantomData<T>,
}

impl QuicConnection<ToClient> {
    pub(crate) fn new(inner: server::Inner) -> Self {
        let (message_send, message_recv) = mpsc::unbounded_channel::<Message>();
        let stream_map: AsyncStreamMap = Arc::new(Mutex::new(HashMap::new()));
        let (incoming_send, incoming_recv) = mpsc::unbounded_channel();

        let driver = Driver {
            inner,
            stream_map: stream_map.clone(),
            message_recv,
            message_send: message_send.clone(),
            incoming_send,
        };
        let handle = tokio::spawn(driver);

        Self {
            handle,
            stream_map,
            message_send,
            incoming_recv,
            state: PhantomData,
        }
    }

    #[inline]
    /// Returns `None` if the driver has closed the stream
    pub async fn incoming(&mut self) -> Option<Incoming> {
        Incoming::from_unchecked(self.incoming_recv.recv().await, true)
    }

    /// Opens a new bidi stream to the client.
    ///
    /// # Arguments
    /// * `id`: A 62 bit integer.
    pub async fn bidi(&mut self, id: u64) -> Result<BidiStream> {
        let mut map = self.stream_map.lock().await;
        let id = (id << 2) | 0b01;
        if map.contains_key(&id) {
            return Err(super::error::Error::IdAlreadyTaken(id));
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let stream = BidiStream {
            id,
            rx,
            tx: self.message_send.clone(),
            buffer_read: BytesMut::with_capacity(u16::MAX as usize),
        };
        map.insert(id, tx);
        Ok(stream)
    }

    /// Opens a new uni stream to the client.
    ///
    /// # Arguments
    /// * `id`: A 62 bit integer.
    pub async fn uni(&mut self, id: u64) -> Result<UniStream<Writeable>> {
        let mut map = self.stream_map.lock().await;
        let id = (id << 2) | 0b11;
        if map.contains_key(&id) {
            return Err(super::error::Error::IdAlreadyTaken(id));
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let stream = UniStream::new(id, rx, self.message_send.clone());
        map.insert(id, tx);
        Ok(stream)
    }
}

impl QuicConnection<ToServer> {
    pub(crate) fn new(inner: client::Inner) -> Self {
        let (message_send, message_recv) = mpsc::unbounded_channel::<Message>();
        let stream_map: AsyncStreamMap = Arc::new(Mutex::new(HashMap::new()));
        let (incoming_send, incoming_recv) = mpsc::unbounded_channel();

        let driver = Driver {
            inner,
            stream_map: stream_map.clone(),
            message_recv,
            message_send: message_send.clone(),
            incoming_send,
        };
        let handle = tokio::spawn(driver);

        Self {
            handle,
            stream_map,
            message_send,
            incoming_recv,
            state: PhantomData,
        }
    }

    #[inline]
    /// Returns `None` if the driver has closed the stream
    pub async fn incoming(&mut self) -> Option<Incoming> {
        Incoming::from_unchecked(self.incoming_recv.recv().await, false)
    }

    /// Opens a new bidi stream to the server.
    ///
    /// # Arguments
    /// * `id`: A 62 bit integer.
    pub async fn bidi(&mut self, id: u64) -> Result<BidiStream> {
        let mut map = self.stream_map.lock().await;
        let id = id << 2;
        if map.contains_key(&id) {
            return Err(super::error::Error::IdAlreadyTaken(id));
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let stream = BidiStream {
            id,
            rx,
            tx: self.message_send.clone(),
            buffer_read: BytesMut::with_capacity(u16::MAX as usize),
        };
        map.insert(id, tx);
        trace!("New bidi stream: {}", stream.id);
        Ok(stream)
    }

    /// Opens a new uni stream to the server.
    ///
    /// # Arguments
    /// * `id`: A 62 bit integer.
    pub async fn uni(&mut self, id: u64) -> Result<UniStream<Writeable>> {
        let mut map = self.stream_map.lock().await;
        let id = (id << 2) | 0b10;
        if map.contains_key(&id) {
            return Err(super::error::Error::IdAlreadyTaken(id));
        }
        let (tx, rx) = mpsc::unbounded_channel();
        let stream = UniStream::new(id, rx, self.message_send.clone());
        map.insert(id, tx);
        trace!("New uni stream: {}", stream.id);
        Ok(stream)
    }
}
