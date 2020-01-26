//! UDP based networking
//!
//! # Packet format
//!
//! The protocol uses the little endian byte-order.
//!
//! ## Normal
//!
//! Normal packets are sent straight down the write with no attempt
//! to ensure they actually reach the other side. This should be taken
//! into account when using this style of packet.
//!
//! They packets must be smaller than 1200 bytes in size
//! due to MTU size limits. Some networks allow more than this
//! but this makes no attempt to detect the increase bandwidth of
//! these networks and instead just targets the smallest.
//!
//! Packets are prepended with a CRC32 checksum which is the sum
//! of the string `UNIVERCITY` followed by the bytes of the serialized
//! packet.
//!
//! ## 'Ensured' packets
//!
//! These packets are wrapped with a header and are tracked to make sure
//! they actually reach the target. This packets also do not have the
//! 1200 byte limit that normal packets have but instead have a 16Kb limit.
//!
//! The system will split up these packets when they are over 1000 bytes and
//! split them into fragments which will be assembled on the overside.
//! The other side will ack the fragments it recieves and the system will
//! resend fragments they did not send.

use super::*;
use std::net::{
    self,
    SocketAddr,
    UdpSocket as NetUdpSocket,
};
use std::fmt::{self, Debug};
use std::thread;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::io;
use std::time::Duration;
use std::cmp;
use byteorder::{ReadBytesExt, WriteBytesExt, LittleEndian};

use crc::{crc32, Hasher32};

use delta_encode::{bitio, DeltaEncodable};
use crate::prelude::*;

const MAX_FRAGMENTS_PARTS: usize = ::std::u16::MAX as usize;
const MAX_WAIT_PACKETS: usize = 128;

/// Listens and manages udp connections
pub struct UdpSocketListener {
    new_sockets: mpsc::Receiver<UdpRemoteSocket>,
    drop_send: mpsc::Sender<SocketAddr>,
}

struct SocketReadInfo {
    state: Arc<Mutex<UdpSocketState>>,
    input_send: mpsc::Sender<packet::Packet>,
    output_send: mpsc::Sender<(bool, packet::Packet)>,
}

struct SocketWriteInfo {
    state: Arc<Mutex<UdpSocketState>>,
    addr: SocketAddr,
    output_read: mpsc::Receiver<(bool, packet::Packet)>,
    output_send: mpsc::Sender<(bool, packet::Packet)>,
}

impl SocketListener for UdpSocketListener {
    type Address = SocketAddr;
    type Socket = UdpRemoteSocket;

    /// Opens a listener of the type that listens to the passed address
    fn listen<A: Into<Self::Address>>(log: &Logger, addr: A) -> UResult<Self> {
        let addr = addr.into();
        let socket = NetUdpSocket::bind(addr)?;
        let write_socket = socket.try_clone()?;
        let (send_socket, read_socket) = mpsc::channel();

        // Threads to manage reads/write.
        // Unlike TCP, UDP does not manage connections for
        // us so all messages go through the same socket
        let (send_writer, read_writer) = mpsc::channel();
        let (send_drop, read_drop) = mpsc::channel();
        {
            let log = log.clone();
            thread::spawn(move || {
                let mut buf = [0; 1500];
                let mut active_sockets: FNVMap<SocketAddr, SocketReadInfo> = FNVMap::default();
                loop {
                    for d in read_drop.try_iter() {
                        active_sockets.remove(&d);
                    }
                    let (count, addr) = match socket.recv_from(&mut buf) {
                        Ok(val) => val,
                        Err(_err) => {
                            // Windows can be weird and pretend the socket
                            // is closed when the port doesn't respond to
                            // a ping.s
                            continue;
                        },
                    };
                    let read_data = &buf[..count];
                    let remove = {
                        // Get the clients write channel creating a new client if
                        // one doesn't already exist.
                        // TODO: Should this be limited somehow? Might be abusable
                        let sinfo = active_sockets.entry(addr).or_insert_with(|| {
                            let (input_send, input_read) = mpsc::channel();
                            let (output_send, output_read) = mpsc::channel();
                            let info = SocketReadInfo {
                                input_send,
                                output_send: output_send.clone(),
                                state: Arc::new(Mutex::new(UdpSocketState::new())),
                            };
                            assume!(log, send_writer.send(SocketWriteInfo {
                                addr,
                                state: info.state.clone(),
                                output_read,
                                output_send: output_send.clone(),
                            }));
                            assume!(log, send_socket.send(UdpRemoteSocket {
                                addr,
                                input_read,
                                output_send,
                            }));
                            info
                        });
                        match packet_from_bytes(read_data)
                            .and_then(|v| handle_packet(&sinfo.state, &sinfo.output_send, v))
                        {
                            Ok(Some(val)) => {
                                sinfo.input_send.send(val).is_err()
                            },
                            Ok(None) => false,
                            Err(e) => {
                                error!(log, "Failed to decode packet: {}", e);
                                true
                            }
                        }
                    };
                    if remove {
                        active_sockets.remove(&addr);
                    }
                }
            });
        }

        let sockets = Arc::new(Mutex::new(vec![]));
        let socks = sockets.clone();
        {
            let log = log.clone();
            thread::spawn(move || {
                loop {
                    let mut sockets = assume!(log, socks.lock());
                    if let Ok(new_socket) = read_writer.try_recv() {
                        sockets.push(new_socket);
                    }
                    let mut should_sleep = true;
                    sockets.retain(|socket| {
                        if let Ok((ensure, pck)) = socket.output_read.try_recv() {
                            should_sleep = false;
                            if write_to(&write_socket, socket.addr, &socket.state, pck, ensure).is_err() {
                                return false;
                            }
                        }
                        true
                    });
                    drop(sockets);
                    if should_sleep {
                        thread::sleep(Duration::from_millis(4));
                    }
                }
            });
        }

        let log = log.clone();
        thread::spawn(move || {
            loop {
                {
                    let sockets = assume!(log, sockets.lock());
                    for socket in sockets.iter() {
                        let mut state = assume!(log, socket.state.lock());
                        if state.monitor(&socket.output_send).is_err() {
                            // Connection dead, kill the monitor thread
                            return;
                        }
                    }
                }
                thread::sleep(Duration::from_millis(20));
            }
        });

        Ok(UdpSocketListener {
            new_sockets: read_socket,
            drop_send: send_drop,
        })
    }

    /// Returns a socket if a connection is queued or
    /// `None` if no additional connections have happened
    /// between now and the last call
    fn next_socket(&mut self) -> Option<Self::Socket> {
        self.new_sockets.try_recv().ok()
    }
    fn format_address(addr: &Self::Address) -> String {
        addr.to_string()
    }

    fn drop_socket(&mut self, id: &<Self::Socket as Socket>::Id) {
        let _ = self.drop_send.send(id.clone());
    }
}

fn write_to(
        socket: &NetUdpSocket, addr: SocketAddr,
        state: &Mutex<UdpSocketState>, pck: packet::Packet,
        ensure: bool
) -> UResult<()> {
    if ensure {
        for pck in ensure_packet(state, pck)? {
            let data = packet_to_bytes(pck, 1200)?;
            socket.send_to(&data, addr)?;
        }
    } else {
        let data = packet_to_bytes(pck, 1200)?;
        socket.send_to(&data, addr)?;
    }
    Ok(())
}

fn write(
        socket: &NetUdpSocket,
        state: &Mutex<UdpSocketState>, pck: packet::Packet,
        ensure: bool
) -> UResult<()> {
    if ensure {
        for pck in ensure_packet(state, pck)? {
            let data = packet_to_bytes(pck, 1200)?;
            socket.send(&data)?;
        }
    } else {
        let data = packet_to_bytes(pck, 1200)?;
        socket.send(&data)?;
    }
    Ok(())
}

/// Udp based socket
pub struct UdpRemoteSocket {
    addr: SocketAddr,
    input_read: mpsc::Receiver<packet::Packet>,
    output_send: mpsc::Sender<(bool, packet::Packet)>,
}

impl Debug for UdpRemoteSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "UdpSocket({:?})", self.addr)
    }
}

impl Socket for UdpRemoteSocket {
    /// The type of a unique hashable id for the socket
    type Id = SocketAddr;

    /// Returns whether this connection is local.
    /// Generally used to reduce auth requirements to
    /// levels that would be unsafe for a non-local server.
    fn is_local() -> bool {
        false
    }
    fn needs_verify() -> bool { true }

    /// Returns the unique id for this connection
    fn id(&mut self) -> Self::Id {
        self.addr
    }

    fn split(self, _log: &Logger) -> (Sender, Receiver) {
        (Sender::Unreliable {
            inner: self.output_send,
        }, Receiver {
            inner: self.input_read
        })
    }
}

/// Udp based socket
pub struct UdpClientSocket {
    addr: SocketAddr,
    socket: NetUdpSocket,
}

impl UdpClientSocket {
    /// Creates a udp socket client
    pub fn connect(addr: SocketAddr) -> UResult<UdpClientSocket> {
        let socket = NetUdpSocket::bind(match addr {
            SocketAddr::V4(_) =>
                SocketAddr::V4(net::SocketAddrV4::new(net::Ipv4Addr::new(0, 0, 0, 0), 0)),
            SocketAddr::V6(_) =>
                SocketAddr::V6(net::SocketAddrV6::new(net::Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 0), 0, 0, 0)),
        })?;
        socket.connect(addr)?;
        Ok(UdpClientSocket {
            addr,
            socket,
        })
    }
}

impl Debug for UdpClientSocket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(f, "UdpSocket({:?})", self.addr)
    }
}

impl Socket for UdpClientSocket {
    /// The type of a unique hashable id for the socket
    type Id = SocketAddr;

    fn is_local() -> bool {
        false
    }
    fn needs_verify() -> bool { true }

    /// Returns the unique id for this connection
    fn id(&mut self) -> Self::Id {
        self.addr
    }

    fn split(self, log: &Logger) -> (Sender, Receiver) {
        let socket = self.socket;
        let write_socket = assume!(log, socket.try_clone());

        let state = Arc::new(Mutex::new(UdpSocketState::new()));

        let (input_send, input_read) = mpsc::channel();
        let (output_send, output_read) = mpsc::channel();

        let os = output_send.clone();
        let st = state.clone();
        let addr = self.addr;
        let log = log.new(o!(
            "client_addr" => addr.to_string(),
        ));
        {
            let log = log.clone();
            thread::spawn(move || {
                let mut buf = [0; 1500];
                while let Ok(count) = socket.recv(&mut buf) {
                    let read_data = &buf[..count];
                    match packet_from_bytes(read_data).and_then(|v| handle_packet(&st, &os, v)) {
                        Ok(Some(val)) => {
                            if input_send.send(val).is_err() {
                                break;
                            }
                        },
                        Ok(None) => {},
                        Err(e) => {
                            error!(log, "Failed to decode packet: {}", e);
                            break
                        }
                    }
                }
            });
        }

        let st = state.clone();
        thread::spawn(move || {
            while let Ok((ensure, pck)) = output_read.recv() {
                if write(&write_socket, &st, pck, ensure).is_err() {
                    break;
                }
            }
        });

        let os = output_send.clone();
        thread::spawn(move || {
            loop {
                {
                    let mut state = assume!(log, state.lock());
                    if state.monitor(&os).is_err() {
                        // Connection closed, kill the monitor thread
                        return;
                    }
                }
                thread::sleep(Duration::from_millis(20));
            }
        });

        (Sender::Unreliable {
            inner: output_send,
        }, Receiver {
            inner: input_read
        })
    }
}

/// The internal state used for tracking fragmented packets
struct UdpSocketState {
    sent_packets: Vec<Option<SentData>>,
    recv_packets: Vec<Option<RecvData>>,
    recv_last_ids: Vec<u16>,
    next_id: u16,
}

impl UdpSocketState {
    fn new() -> UdpSocketState {
        UdpSocketState {
            sent_packets: vec![None; MAX_WAIT_PACKETS],
            recv_packets: vec![None; MAX_WAIT_PACKETS],
            recv_last_ids: vec![0xFFFF; MAX_WAIT_PACKETS],
            next_id: 0,
        }
    }

    // Resends packets that the other side hasn't ack'd yet.
    fn monitor(&mut self, output_send: &mpsc::Sender<(bool, packet::Packet)>) -> UResult<()> {
        for slot in &mut self.sent_packets {
            if let Some(slot) = slot.as_mut() {
                slot.last_resend -= 1;
                if slot.last_resend != 0 {
                    continue;
                }
                // Back off each time as the rate this is being
                // sent might be the reason the packet was dropped.
                slot.resend_count += 1;
                slot.last_resend = 4 * slot.resend_count;

                for part in 0 .. slot.fragments as usize {
                    if slot.recv_bits.get(part) {
                        continue;
                    }
                    let offset = part * 1000;
                    // Send in 1000 byte chunks (or the remaining data
                    // if less than 1000)
                    let size = cmp::min(slot.data.len() - offset, 1000);
                    // Copy the data into a new packet
                    let data = slot.data[offset .. offset + size].to_vec();
                    let wrapper = packet::Ensured {
                        fragment_id: slot.id,
                        fragment_part: part as u16,
                        fragment_max_parts: (slot.fragments - 1) as u16,
                        internal_packet: packet::Raw(data),
                    };
                    // Send normally
                    if output_send.send((false, wrapper.into())).is_err() {
                        bail!(ErrorKind::ConnectionClosed);
                    }
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct SentData {
    id: u16,
    recv_bits: BitSet,
    mask: BitSet,
    fragments: u16,
    data: Vec<u8>,
    last_resend: i32,
    resend_count: i32,
}

#[derive(Clone)]
struct RecvData {
    id: u16,
    recv_bits: BitSet,
    mask: BitSet,
    fragments: u16,
    data: Vec<u8>,
    len: usize,
}

// Attempts to serialize a packet as a vector.
pub(super) fn packet_to_bytes(packet: packet::Packet, limit: usize) -> UResult<Vec<u8>> {
    let mut writer = bitio::Writer::new(Vec::with_capacity(500));
    packet.encode(None, &mut writer)?;

    let mut buf = writer.finish()?;
    // Sane max packet size limit
    assert!(buf.len() <= limit, "Packet larger than limit {:?}", packet);

    // Doesn't prevent too much but helps make sure we are actually
    // talking to a univercity server and not something else running
    // on the same port.
    let mut digest = crc32::Digest::new(crc32::IEEE);
    digest.write(b"UNIVERCITY");
    digest.write(&buf);

    let mut out = Vec::with_capacity(4 + buf.len());
    out.write_u32::<LittleEndian>(digest.sum32())?;
    out.append(&mut buf);
    Ok(out)
}

fn ensure_packet(state: &Mutex<UdpSocketState>, packet: packet::Packet) -> UResult<Vec<packet::Packet>> {
    let mut writer = bitio::Writer::new(Vec::with_capacity(500));
    packet.encode(None, &mut writer)?;
    let buf = writer.finish()?;

    let num_fragments = (buf.len() + 999) / 1000;
    if num_fragments > MAX_FRAGMENTS_PARTS {
        return Err(ErrorKind::PacketTooLarge.into());
    }

    // Grab an id that should be unique to this packet (within a
    // given timeframe).
    let mut state = state.lock().map_err(|_| ErrorKind::Msg("Lock failed".into()))?;
    let frag_id = state.next_id;
    state.next_id = state.next_id.wrapping_add(1);
    // If the slot is taken then the other side hasn't responded to
    // in a while. Assume something is wrong and error.
    let send_slot = match state.sent_packets.get_mut(frag_id as usize % MAX_WAIT_PACKETS) {
        Some(val) => val,
        None => return Err(ErrorKind::NoPacketSlots.into()),
    };

    let mut out_list = vec![];

    // Split up the packet into 1000 byte fragments and send
    let mut mask = BitSet::new(num_fragments as usize);
    for part in 0 .. num_fragments {
        mask.set(part, true);
        let offset = part * 1000;
        let size = cmp::min(buf.len() - offset, 1000);
        let data = buf[offset .. offset + size].to_vec();
        let wrapper = packet::Ensured {
            fragment_id: frag_id,
            fragment_part: part as u16,
            fragment_max_parts: (num_fragments - 1) as u16,
            internal_packet: packet::Raw(data),
        };
        // Send raw
        out_list.push(wrapper.into());
    }
    *send_slot = Some(SentData {
        id: frag_id,
        recv_bits: BitSet::new(num_fragments as usize),
        mask,
        data: buf,
        fragments: num_fragments as u16,
        last_resend: 4,
        resend_count: 0,
    });
    Ok(out_list)
}

pub(super) fn packet_from_bytes(data: &[u8]) -> UResult<packet::Packet> {
    if data.len() <= 4 {
        return Err(ErrorKind::DataTooSmall.into());
    }
    // Attempt to verify this packet was from a univercity client/server.
    // Wont always been right but its good enough.
    let mut digest = crc32::Digest::new(crc32::IEEE);
    digest.write(b"UNIVERCITY");
    digest.write(&data[4..]);

    let mut cur = io::Cursor::new(data);
    let crc = cur.read_u32::<LittleEndian>()?;
    if crc != digest.sum32() {
        return Err(ErrorKind::ChecksumMismatch(digest.sum32(), crc).into());
    }
    // Decode the packet
    let mut r = bitio::Reader::new(cur);
    let packet = packet::Packet::decode(None, &mut r)?;
    Ok(packet)
}

fn handle_packet(state: &Mutex<UdpSocketState>, send: &mpsc::Sender<(bool, packet::Packet)>, pck: packet::Packet) -> UResult<Option<packet::Packet>> {
    use std::mem;
    use std::cmp::min;
    Ok(match pck {
        packet::Packet::EnsuredAck(pck) => {
            let mut state = state.lock().map_err(|_| ErrorKind::Msg("Lock failed".into()))?;
            let done = {
                let slot = state.sent_packets.get_mut(pck.fragment_id as usize % MAX_WAIT_PACKETS).unwrap();
                if let Some(slot) = slot.as_mut() {
                    if slot.id != pck.fragment_id {
                        // Ignore
                        return Ok(None);
                    }
                    // Copy their mask on to ours with an OR.
                    // This way we don't unmark bits when we recvive a
                    // late packet
                    slot.recv_bits.set(pck.fragment_part as usize, true);
                    slot.recv_bits.includes_set(&slot.mask)
                } else {
                    // Ignore
                    return Ok(None);
                }
            };
            if done {
                // Stop watching for the packet
                state.recv_packets[pck.fragment_id as usize % MAX_WAIT_PACKETS] = None;
            }
            None
        },
        packet::Packet::Ensured(pck) => {
            let mut lock = state.lock().map_err(|_| ErrorKind::Msg("Lock failed".into()))?;
            let state: &mut UdpSocketState = &mut *lock;
            let done = {
                let slot = state.recv_packets.get_mut(pck.fragment_id as usize % MAX_WAIT_PACKETS).unwrap();
                // Empty slot, fill it
                if slot.is_none() {
                    if state.recv_last_ids[pck.fragment_id as usize % MAX_WAIT_PACKETS] == pck.fragment_id {
                        // Echo, ignore it
                        return Ok(None);
                    }
                    let mut mask = BitSet::new((pck.fragment_max_parts + 1) as usize);
                    for i in 0 .. (pck.fragment_max_parts + 1) as usize {
                        mask.set(i, true);
                    }
                    *slot = Some(RecvData {
                        id: pck.fragment_id,
                        recv_bits: BitSet::new((pck.fragment_max_parts + 1) as usize),
                        mask,
                        fragments: pck.fragment_max_parts + 1,
                        data: vec![0; (pck.fragment_max_parts + 1) as usize * 1000],
                        len: 0,
                    });
                    state.recv_last_ids[pck.fragment_id as usize % MAX_WAIT_PACKETS] = pck.fragment_id;
                }
                let slot = slot.as_mut().expect("Slot missing after assignment");
                if slot.id != pck.fragment_id {
                    // Assume this packet in an echo and ignore it
                    return Ok(None);
                }
                if pck.fragment_part >= slot.fragments {
                    return Err(ErrorKind::InvalidFragment.into());
                }
                if pck.fragment_max_parts + 1 != slot.fragments {
                    return Err(ErrorKind::MaxFragmentPartChanged.into());
                }
                if pck.internal_packet.0.len() > 1000 + 4 {
                    return Err(ErrorKind::DataTooLarge.into());
                }
                // Have we already handled this fragment?
                if !slot.recv_bits.get(pck.fragment_part as usize) {
                    slot.recv_bits.set(pck.fragment_part as usize, true);
                    // Copy the data into our buffer
                    let part = pck.fragment_part as usize;
                    let end = min(min(slot.data.len(), (part + 1) * 1000) - (part * 1000), pck.internal_packet.0.len());
                    // Last fragment, use as the size
                    if pck.fragment_part == slot.fragments - 1 {
                        slot.len = part * 1000 + end;
                    }
                    slot.data[part * 1000 .. part * 1000 + end].copy_from_slice(&pck.internal_packet.0[.. end]);
                }
                // Tell the other side again that we got the packet even if
                // we ignored it encase the ack was dropped
                send.send((false, packet::EnsuredAck {
                    fragment_id: slot.id,
                    fragment_part: pck.fragment_part,
                }.into())).map_err(|_| ErrorKind::ConnectionClosed)?;
                slot.recv_bits.includes_set(&slot.mask)
            };
            if done {
                // We have the whole packet now. Parse it and return it
                let mut slot = mem::replace(&mut state.recv_packets[pck.fragment_id as usize % MAX_WAIT_PACKETS], None)
                    .expect("Missing slot when recreating packet");
                slot.data.truncate(slot.len);
                let cur = io::Cursor::new(slot.data);
                let mut r = bitio::Reader::new(cur);
                Some(packet::Packet::decode(None, &mut r)?)
            } else {
                None
            }
        },
        pck => Some(pck),
    })
}

#[test]
fn test_udp() {
    let log = ::slog::Logger::root(::slog::Discard, o!());
    let addr: SocketAddr = "127.0.0.1:23349".parse().unwrap();
    let mut listen = UdpSocketListener::listen(&log, addr).unwrap();

    let client = UdpClientSocket::connect(addr).unwrap();
    let (mut send, _read) = client.split(&log);
    send.send(packet::ServerConnectionFail {
        reason: "Testing 1 2 3".into()
    }).unwrap();

    // Prevent races
    thread::sleep(::std::time::Duration::from_millis(75));

    let client_remote = listen.next_socket().unwrap();
    let (_remote_send, mut remote_read) = client_remote.split(&log);
    let packet = remote_read.recv_timeout(time::Duration::from_secs(5)).unwrap();
    let packet = if let packet::Packet::ServerConnectionFail(pck) = packet {
        pck
    } else {
        panic!("Wrong packet");
    };
    assert_eq!("Testing 1 2 3", packet.reason);
}

#[test]
fn test_udp_ensure() {
    let log = ::slog::Logger::root(::slog::Discard, o!());
    let addr: SocketAddr = "127.0.0.1:23348".parse().unwrap();
    let mut listen = UdpSocketListener::listen(&log, addr).unwrap();

    let msg: String = ::std::iter::repeat("Testing 1 2 3").take(600).collect();

    let client = UdpClientSocket::connect(addr).unwrap();
    let (mut send, _read) = client.split(&log);
    send.ensure_send(packet::ServerConnectionFail {
        reason: msg.clone(),
    }).unwrap();

    // Prevent races
    thread::sleep(::std::time::Duration::from_millis(75));

    let client_remote = listen.next_socket().unwrap();
    let (_remote_send, mut remote_read) = client_remote.split(&log);
    let packet = remote_read.recv_timeout(time::Duration::from_secs(5)).unwrap();
    let packet = if let packet::Packet::ServerConnectionFail(pck) = packet {
        pck
    } else {
        panic!("Wrong packet");
    };
    assert_eq!(msg, packet.reason);
}
