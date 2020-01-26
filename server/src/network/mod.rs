//! Network handling

#[macro_use]
pub mod packet;

pub mod local;
pub use self::local::*;

pub mod udp;
pub use self::udp::*;

#[cfg(feature = "steam")]
pub mod steam;
#[cfg(feature = "steam")]
pub use self::steam::*;

use std::sync::mpsc;
use std::fmt::Debug;
use std::hash::Hash;
use std::collections::hash_map::ValuesMut;
use std::time;
use std::marker::PhantomData;
use std::cell::RefCell;
use crate::event;

use crate::util::FNVMap;
use crate::errors;
use crate::prelude::*;

/// Manages network connections
pub struct NetworkManager<S: SocketListener> {
    log: Logger,
    listener: S,
    connections: FNVMap<<S::Socket as Socket>::Id, Connection<S::Socket>>,
}

impl <S: SocketListener> NetworkManager<S> {
    /// Creates a new network manager that listens for connections on the passed
    /// address
    pub fn new<A>(log: &Logger, addr: A) -> errors::Result<NetworkManager<S>>
        where A: Into<<S as SocketListener>::Address>
    {
        let log = log.new(o!(
            "source" => "network-manager",
        ));
        Ok(NetworkManager {
            listener: S::listen(&log, addr)?,
            connections: Default::default(),
            log,
        })
    }

    /// Ticks the network manager, checking for new connections and
    /// handling them.
    pub fn tick(&mut self) {
        while let Some(mut socket) = self.listener.next_socket() {
            info!(self.log, "New connection: {:?}", socket);
            let id = socket.id();
            self.connections.insert(id.clone(), Connection::new(&self.log, socket));
        }
    }

    /// Returns the connection with the given id if it exists
    pub fn get_connection(&mut self, id: &<S::Socket as Socket>::Id) -> Option<&mut Connection<S::Socket>> {
        self.connections.get_mut(id)
    }

    /// Returns all currently active connections.
    pub fn connections(&mut self) -> ValuesMut<'_, <S::Socket as Socket>::Id, Connection<S::Socket>> {
        let listener = &mut self.listener;
        self.connections.retain(|_, v| if v.closed {
            listener.drop_socket(&v.id);
            false
        } else {
            true
        });
        self.connections.values_mut()
    }

    /// Returns whether a connection with the given id exists
    pub fn is_connection_open(&self, id: &<S::Socket as Socket>::Id) -> bool {
        self.connections.contains_key(id)
    }

    /// Returns the host (if any) of the server
    pub fn get_host(&self) -> Option<<S::Socket as Socket>::Id> {
        self.listener.host()
    }
}

/// A single socket connection
pub struct Connection<S: Socket> {
    /// This connection's ID
    pub id: S::Id,
    closed: bool,
    send: Sender,
    recv: Receiver,
}

impl <S: Socket> Connection<S> {
    fn new(log: &Logger, mut socket: S) -> Connection<S> {
        let id = socket.id();
        let (send, recv) = socket.split(log);
        Connection {
            id,
            closed: false,
            send,
            recv,
        }
    }

    /// Forces the socket to close
    pub fn close(&mut self) {
        self.closed = true;
    }

    /// Attempts to send a packet to the target.
    /// Order of the frames when recieved by the target and
    /// whether the data arrives at all isn't guaranteed.
    pub fn send<P>(&mut self, data: P) -> errors::Result<()>
        where P: Into<packet::Packet> + Debug
    {
        let ret = self.send.send(data);
        self.closed |= ret.is_err();
        ret
    }

    /// Sends a single packet to the target. Order of the
    /// when recieved isn't guaranteed but the frame will arrive
    /// assuming no long-term issues.
    ///
    /// If the frame is failed to be sent within a implementation
    /// defined window then the socket should be closed and an
    /// error returned for all future `send*` and `recv` calls.
    pub fn ensure_send<P>(&mut self, data: P) -> errors::Result<()>
        where P: Into<packet::Packet> + Debug
    {
        let ret = self.send.ensure_send(data);
        self.closed |= ret.is_err();
        ret
    }

    /// Reads a single Packet if available.
    pub fn recv(&mut self) -> errors::Result<packet::Packet> {
        let ret = self.recv.try_recv();
        self.closed |= ret.as_ref()
            .err()
            .map_or(false, |e| if let errors::ErrorKind::NoData = *e.kind() {
                false
            } else {
                true
            });
        ret
    }
}

impl NetworkManager<LoopbackSocketListener> {
    /// Returns the socket to be used by the local client
    pub fn client_localsocket(&mut self) -> LoopbackSocket {
        assume!(self.log, self.listener.client.take())
    }
}

#[cfg(feature = "steam")]
impl NetworkManager<SteamSocketListener> {
    /// Returns the socket to be used by the local client
    pub fn client_localsocket(&mut self) -> SteamSocket {
        assume!(self.log, self.listener.client.take())
    }
}

/// Indirect acess to the send half of a
/// connection.
pub enum Sender {
    /// A sender that wont fail whilst the connection is active
    Reliable {
        #[doc(hidden)]
        inner: mpsc::Sender<packet::Packet>,
    },
    /// A sender that can fail to send a packet when the bool
    /// is false.
    Unreliable {
        #[doc(hidden)]
        inner: mpsc::Sender<(bool, packet::Packet)>,
    },
}

impl Sender {
    /// Attempts to send a packet to the target.
    /// Order of the frames when recieved by the target and
    /// whether the data arrives at all isn't guaranteed.
    pub fn send<P>(&mut self, data: P) -> errors::Result<()>
        where P: Into<packet::Packet>
    {
        match *self {
            Sender::Reliable{ref inner} => inner.send(data.into())
                .map_err(|_| errors::ErrorKind::ConnectionClosed.into()),
            Sender::Unreliable{ref inner} => inner.send((false, data.into()))
                .map_err(|_| errors::ErrorKind::ConnectionClosed.into()),
        }
    }

    /// Sends a single packet to the target. Order of the
    /// when recieved isn't guaranteed but the frame will arrive
    /// assuming no long-term issues.
    ///
    /// If the frame is failed to be sent within a implementation
    /// defined window then the socket should be closed and an
    /// error returned for all future `send*` and `recv` calls.
    pub fn ensure_send<P>(&mut self, data: P) -> errors::Result<()>
        where P: Into<packet::Packet>
    {
        match *self {
            Sender::Reliable{ref inner} => inner.send(data.into())
                .map_err(|_| errors::ErrorKind::ConnectionClosed.into()),
            Sender::Unreliable{ref inner} => inner.send((true, data.into()))
                .map_err(|_| errors::ErrorKind::ConnectionClosed.into()),
        }
    }
}

/// Indirect acess to the recieve half of a
/// connection.
pub struct Receiver {
    inner: mpsc::Receiver<packet::Packet>,
}

impl Receiver {

    /// Reads a single Packet if available.
    pub fn try_recv(&mut self) -> errors::Result<packet::Packet> {
        let ret = self.inner.try_recv();
        ret.map_err(|e| match e {
            mpsc::TryRecvError::Disconnected => errors::ErrorKind::ConnectionClosed.into(),
            mpsc::TryRecvError::Empty => errors::ErrorKind::NoData.into(),
        })
    }

    /// Reads a single Packet if available.
    pub fn recv_timeout(&mut self, time: time::Duration) -> errors::Result<packet::Packet> {
        let ret = self.inner.recv_timeout(time);
        ret.map_err(|_| errors::ErrorKind::ConnectionClosed.into())
    }
}

/// A single socket
pub trait Socket: Debug + Sized {
    /// The type of a unique hashable id for the socket
    type Id: PartialEq + Eq + Hash + Clone + Debug + 'static;

    /// Returns whether this connection is local.
    /// Generally used to reduce auth requirements to
    /// levels that would be unsafe for a non-local server.
    fn is_local() -> bool;
    /// Returns whether this connection needs to verify
    /// connecting users.
    ///
    /// For some protocols this may be done for us.
    fn needs_verify() -> bool;

    /// Returns the unique id for this connection
    fn id(&mut self) -> Self::Id;

    /// Splits the socket into two halfs
    fn split(self, log: &Logger) -> (Sender, Receiver);
}

/// Listens for socket connections and passes them back to the manager
pub trait SocketListener: Sized {
    /// The type of address this listener will accept
    type Address;
    /// The type of socket that will be returned
    type Socket: Socket;

    /// Opens a listener of the type that listens to the passed address
    fn listen<A: Into<Self::Address>>(log: &Logger, addr: A) -> errors::Result<Self>;

    /// Returns a socket if a connection is queued or
    /// `None` if no additional connections have happened
    /// between now and the last call
    fn next_socket(&mut self) -> Option<Self::Socket>;

    /// Tells the listener to drop the socket with the given id
    fn drop_socket(&mut self, _id: &<Self::Socket as Socket>::Id) {}

    /// Creates a printable string of the address
    fn format_address(addr: &Self::Address) -> String;

    /// Returns the host (if any) of the listener
    fn host(&self) -> Option<<Self::Socket as Socket>::Id> {
        None
    }
}

/// A helper for simple request/reply type of messages
pub struct RequestManager {
    /// A list of packets to send
    packets: Vec<packet::Packet>,

    requests: Vec<packet::Request>,
    replies: RefCell<Vec<packet::Packet>>,

    next_id: u32,
}

/// A requestable type
pub trait Requestable: delta_encode::DeltaEncodable {
    /// A unique id for this request type
    const ID: [u8; 4];

    /// The type that is sent as a reply
    type Reply: delta_encode::DeltaEncodable;
}

/// A ticket given when a request is made
pub struct RequestTicket<R> {
    _r: PhantomData<R>,
    id: u32,
}

impl <R> Clone for RequestTicket<R> {
    fn clone(&self) -> Self {
        Self {
            _r: PhantomData,
            id: self.id,
        }
    }
}

impl <R> Copy for RequestTicket<R> {
}

/// An event sent when a reply is given
pub struct ReplyEvent(pub packet::Reply);

struct Requests<'a, I: 'a> {
    packets: &'a mut Vec<packet::Packet>,
    replies: &'a RefCell<Vec<packet::Packet>>,
    iter: I,
}

/// An undecoded request
pub struct AnyRequest<'a> {
    reply: &'a RefCell<Vec<packet::Packet>>,
    pck: packet::Request,
}

/// Helper used to send replies to requests
pub struct Replier<'a, R> {
    id: u32,
    reply: &'a RefCell<Vec<packet::Packet>>,
    _r: PhantomData<R>,
}

impl <'a, R> Replier<'a, R>
    where R: Requestable
{
    /// Replies to the request
    pub fn reply(&self, rpl: R::Reply) {
        use delta_encode::DeltaEncodable;
        let replies = &mut *self.reply.borrow_mut();

        let mut data = bitio::Writer::new(Vec::new());
        rpl.encode(None, &mut data).expect("Failed to encode reply");
        replies.push(packet::Reply {
            ty: R::ID,
            id: self.id,
            data: packet::Raw(data.finish().expect("Failed to encode reply")),
        }.into());
    }
}

impl <'a> AnyRequest<'a> {
    /// Tries to handle the request as the given type
    pub fn handle<R, F>(&self, func: F)
        where R: Requestable,
            F: FnOnce(R, Replier<'a, R>)
    {
        use std::io;
        if self.pck.ty == R::ID {
            let mut reader = bitio::Reader::new(io::Cursor::new(&self.pck.data.0));
            let r = R::decode(None, &mut reader).expect("Failed to decode request");
            func(r, Replier {
                id: self.pck.id,
                reply: self.reply,
                _r: PhantomData,
            });
        }
    }
}

impl <'a, I> Iterator for Requests<'a, I>
    where I: Iterator<Item=packet::Request>,
{
    type Item = AnyRequest<'a>;

    fn next(&mut self) -> Option<AnyRequest<'a>> {
        let req = self.iter.next()?;
        Some(AnyRequest {
            reply: self.replies,
            pck: req,
        })
    }
}

impl <'a, I> Drop for Requests<'a, I> {
    fn drop(&mut self) {
        let mut r = self.replies.borrow_mut();
        self.packets.append(&mut *r);
    }
}

impl RequestManager {
    /// Creates a new request manager
    pub fn new() -> RequestManager {
        RequestManager {
            packets: Vec::new(),
            requests: Vec::new(),
            replies: RefCell::new(Vec::new()),
            next_id: 0,
        }
    }

    /// Handles incoming request packets
    pub fn parse_request(&mut self, req: packet::Request) {
        self.requests.push(req);
    }

    /// Returns a collection of packets that need to be sent
    pub fn packets(&mut self) -> impl Iterator<Item=packet::Packet> + '_ {
        self.packets.drain(..)
    }

    /// Queues a request to be sent
    pub fn request<R>(&mut self, req: R) -> RequestTicket<R>
        where R: Requestable
    {
        let mut data = bitio::Writer::new(Vec::new());
        req.encode(None, &mut data).expect("Failed to encode request");
        self.packets.push(packet::Request {
            ty: R::ID,
            id: self.next_id,
            data: packet::Raw(data.finish().expect("Failed to encode request")),
        }.into());
        let ticket = RequestTicket {
            _r: PhantomData,
            id: self.next_id,
        };
        self.next_id = self.next_id.wrapping_add(1);
        ticket
    }

    /// Waits for a reply and calls the function if the event
    /// is the reply
    pub fn handle_reply<R, F>(evt: &mut event::EventHandler, ticket: RequestTicket<R>, func: F)
        where F: FnOnce(R::Reply),
            R: Requestable
    {
        use std::io;
        use delta_encode::DeltaEncodable;
        evt.handle_event_if::<ReplyEvent, _, _>(
            |evt| evt.0.id == ticket.id && evt.0.ty == R::ID,
            |evt| {
                let mut r = bitio::Reader::new(io::Cursor::new(evt.0.data.0));
                let rpl = R::Reply::decode(None, &mut r).expect("Failed to decode reply");
                func(rpl);
            }
        );
    }

    /// An iterator over requests
    pub fn requests(&mut self) -> impl Iterator<Item=AnyRequest<'_>> {
        Requests {
            packets: &mut self.packets,
            replies: &self.replies,
            iter: self.requests.drain(..),
        }
    }
}