use redox::boxed::Box;
use redox::fs::File;
use redox::io::{Read, Write, SeekFrom};
use redox::mem;
use redox::net::*;
use redox::rand;
use redox::slice;
use redox::string::{String, ToString};
use redox::to_num::*;
use redox::vec::Vec;
use redox::URL;

#[derive(Copy, Clone)]
#[repr(packed)]
pub struct TCPHeader {
    pub src: n16,
    pub dst: n16,
    pub sequence: n32,
    pub ack_num: n32,
    pub flags: n16,
    pub window_size: n16,
    pub checksum: Checksum,
    pub urgent_pointer: n16,
}

pub struct TCP {
    pub header: TCPHeader,
    pub options: Vec<u8>,
    pub data: Vec<u8>,
}

pub const TCP_FIN: u16 = 1;
pub const TCP_SYN: u16 = 1 << 1;
pub const TCP_RST: u16 = 1 << 2;
pub const TCP_PSH: u16 = 1 << 3;
pub const TCP_ACK: u16 = 1 << 4;

impl FromBytes for TCP {
    fn from_bytes(bytes: Vec<u8>) -> Option<Self> {
        if bytes.len() >= mem::size_of::<TCPHeader>() {
            unsafe {
                let header = *(bytes.as_ptr() as *const TCPHeader);
                let header_len = ((header.flags.get() & 0xF000) >> 10) as usize;

                return Some(TCP {
                    header: header,
                    options: bytes[mem::size_of::<TCPHeader>()..header_len].to_vec(),
                    data: bytes[header_len..bytes.len()].to_vec(),
                });
            }
        }
        None
    }
}

impl ToBytes for TCP {
    fn to_bytes(&self) -> Vec<u8> {
        unsafe {
            let header_ptr: *const TCPHeader = &self.header;
            let mut ret = Vec::from(slice::from_raw_parts(header_ptr as *const u8, mem::size_of::<TCPHeader>()));
            ret.push_all(&self.options);
            ret.push_all(&self.data);
            ret
        }
    }
}

/// A TCP resource
pub struct Resource {
    ip: File,
    peer_addr: IPv4Addr,
    peer_port: u16,
    host_port: u16,
    sequence: u32,
    acknowledge: u32,
}

impl Resource {
    pub fn dup(&self) -> Option<Box<Resource>> {
        match self.ip.dup() {
            Some(ip) => Some(box Resource {
                ip: ip,
                peer_addr: self.peer_addr,
                peer_port: self.peer_port,
                host_port: self.host_port,
                sequence: self.sequence,
                acknowledge: self.acknowledge,
            }),
            None => None
        }
    }

    pub fn path(&self) -> Option<String> {
        Some(format!("tcp://{}:{}/{}", self.peer_addr.to_string(), self.peer_port, self.host_port as usize))
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Option<usize> {
        loop {
            let mut bytes: Vec<u8> = Vec::new();
            match self.ip.read_to_end(&mut bytes) {
                Some(_) => {
                    if let Some(segment) = TCP::from_bytes(bytes) {
                        if (segment.header.flags.get() & (TCP_PSH | TCP_SYN | TCP_ACK)) ==
                           (TCP_PSH | TCP_ACK) &&
                           segment.header.dst.get() == self.host_port &&
                           segment.header.src.get() == self.peer_port {
                            //Send ACK
                            self.sequence = segment.header.ack_num.get();
                            self.acknowledge = segment.header.sequence.get() +
                                               segment.data.len() as u32;
                            let mut tcp = TCP {
                                header: TCPHeader {
                                    src: n16::new(self.host_port),
                                    dst: n16::new(self.peer_port),
                                    sequence: n32::new(self.sequence),
                                    ack_num: n32::new(self.acknowledge),
                                    flags: n16::new(((mem::size_of::<TCPHeader>() << 10) & 0xF000) as u16 | TCP_ACK),
                                    window_size: n16::new(65535),
                                    checksum: Checksum {
                                        data: 0
                                    },
                                    urgent_pointer: n16::new(0)
                                },
                                options: Vec::new(),
                                data: Vec::new()
                            };

                            unsafe {
                                let proto = n16::new(0x06);
                                let segment_len = n16::new((mem::size_of::<TCPHeader>() + tcp.options.len() + tcp.data.len()) as u16);
                                tcp.header.checksum.data = Checksum::compile(
                                    Checksum::sum((&IP_ADDR as *const IPv4Addr) as usize, mem::size_of::<IPv4Addr>()) +
                                    Checksum::sum((&self.peer_addr as *const IPv4Addr) as usize, mem::size_of::<IPv4Addr>()) +
                                    Checksum::sum((&proto as *const n16) as usize, mem::size_of::<n16>()) +
                                    Checksum::sum((&segment_len as *const n16) as usize, mem::size_of::<n16>()) +
                                    Checksum::sum((&tcp.header as *const TCPHeader) as usize, mem::size_of::<TCPHeader>()) +
                                    Checksum::sum(tcp.options.as_ptr() as usize, tcp.options.len()) +
                                    Checksum::sum(tcp.data.as_ptr() as usize, tcp.data.len())
                                );
                            }

                            self.ip.write(&tcp.to_bytes());

                            //TODO: Support broken packets (one packet in two buffers)
                            let mut i = 0;
                            while i < buf.len() && i < segment.data.len() {
                                buf[i] = segment.data[i];
                                i += 1;
                            }
                            return Some(i);
                        }
                    }
                }
                None => return None,
            }
        }
    }

    pub fn write(&mut self, buf: &[u8]) -> Option<usize> {
        let tcp_data = Vec::from(buf);

        let mut tcp = TCP {
            header: TCPHeader {
                src: n16::new(self.host_port),
                dst: n16::new(self.peer_port),
                sequence: n32::new(self.sequence),
                ack_num: n32::new(self.acknowledge),
                flags: n16::new((((mem::size_of::<TCPHeader>()) << 10) & 0xF000) as u16 | TCP_PSH |
                                TCP_ACK),
                window_size: n16::new(65535),
                checksum: Checksum { data: 0 },
                urgent_pointer: n16::new(0),
            },
            options: Vec::new(),
            data: tcp_data,
        };

        unsafe {
            let proto = n16::new(0x06);
            let segment_len = n16::new((mem::size_of::<TCPHeader>() + tcp.data.len()) as u16);
            tcp.header.checksum.data =
                Checksum::compile(Checksum::sum((&IP_ADDR as *const IPv4Addr) as usize,
                                                mem::size_of::<IPv4Addr>()) +
                                  Checksum::sum((&self.peer_addr as *const IPv4Addr) as usize,
                                                mem::size_of::<IPv4Addr>()) +
                                  Checksum::sum((&proto as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&segment_len as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&tcp.header as *const TCPHeader) as usize,
                                                mem::size_of::<TCPHeader>()) +
                                  Checksum::sum(tcp.options.as_ptr() as usize, tcp.options.len()) +
                                  Checksum::sum(tcp.data.as_ptr() as usize, tcp.data.len()));
        }

        match self.ip.write(&tcp.to_bytes()) {
            Some(size) => loop { // Wait for ACK
                let mut bytes: Vec<u8> = Vec::new();
                match self.ip.read_to_end(&mut bytes) {
                    Some(_) => {
                        if let Some(segment) = TCP::from_bytes(bytes) {
                            if segment.header.dst.get() == self.host_port &&
                               segment.header.src.get() == self.peer_port {
                                if (segment.header.flags.get() & (TCP_PSH | TCP_SYN | TCP_ACK)) ==
                                   TCP_ACK {
                                    self.sequence = segment.header.ack_num.get();
                                    self.acknowledge = segment.header.sequence.get();
                                    return Some(size);
                                } else {
                                    return None;
                                }
                            }
                        }
                    }
                    None => return None,
                }
            },
            None => return None,
        }
    }

    pub fn seek(&mut self, pos: SeekFrom) -> Option<usize> {
        return None;
    }

    pub fn sync(&mut self) -> bool {
        return self.ip.sync();
    }

    /// Etablish client
    pub fn client_establish(&mut self) -> bool {
        // Send SYN
        let mut tcp = TCP {
            header: TCPHeader {
                src: n16::new(self.host_port),
                dst: n16::new(self.peer_port),
                sequence: n32::new(self.sequence),
                ack_num: n32::new(self.acknowledge),
                flags: n16::new(((mem::size_of::<TCPHeader>() << 10) & 0xF000) as u16 | TCP_SYN),
                window_size: n16::new(65535),
                checksum: Checksum { data: 0 },
                urgent_pointer: n16::new(0),
            },
            options: Vec::new(),
            data: Vec::new(),
        };

        unsafe {
            let proto = n16::new(0x06);
            let segment_len =
                n16::new((mem::size_of::<TCPHeader>() + tcp.options.len() + tcp.data.len()) as u16);
            tcp.header.checksum.data =
                Checksum::compile(Checksum::sum((&IP_ADDR as *const IPv4Addr) as usize,
                                                mem::size_of::<IPv4Addr>()) +
                                  Checksum::sum((&self.peer_addr as *const IPv4Addr) as usize,
                                                mem::size_of::<IPv4Addr>()) +
                                  Checksum::sum((&proto as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&segment_len as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&tcp.header as *const TCPHeader) as usize,
                                                mem::size_of::<TCPHeader>()) +
                                  Checksum::sum(tcp.options.as_ptr() as usize, tcp.options.len()) +
                                  Checksum::sum(tcp.data.as_ptr() as usize, tcp.data.len()));
        }

        match self.ip.write(&tcp.to_bytes()) {
            Some(_) => loop { // Wait for SYN-ACK
                let mut bytes: Vec<u8> = Vec::new();
                match self.ip.read_to_end(&mut bytes) {
                    Some(_) => {
                        if let Some(segment) = TCP::from_bytes(bytes) {
                            if segment.header.dst.get() == self.host_port &&
                               segment.header.src.get() == self.peer_port {
                                if (segment.header.flags.get() & (TCP_PSH | TCP_SYN | TCP_ACK)) ==
                                   (TCP_SYN | TCP_ACK) {
                                    self.sequence = segment.header.ack_num.get();
                                    self.acknowledge = segment.header.sequence.get();

                                    self.acknowledge += 1;
                                    tcp = TCP {
                                        header: TCPHeader {
                                            src: n16::new(self.host_port),
                                            dst: n16::new(self.peer_port),
                                            sequence: n32::new(self.sequence),
                                            ack_num: n32::new(self.acknowledge),
                                            flags: n16::new(((mem::size_of::<TCPHeader>() << 10) & 0xF000) as u16 | TCP_ACK),
                                            window_size: n16::new(65535),
                                            checksum: Checksum {
                                                data: 0
                                            },
                                            urgent_pointer: n16::new(0)
                                        },
                                        options: Vec::new(),
                                        data: Vec::new()
                                    };

                                    unsafe {
                                        let proto = n16::new(0x06);
                                        let segment_len = n16::new((mem::size_of::<TCPHeader>() + tcp.options.len() + tcp.data.len()) as u16);
                                        tcp.header.checksum.data = Checksum::compile(
                                            Checksum::sum((&IP_ADDR as *const IPv4Addr) as usize, mem::size_of::<IPv4Addr>()) +
                                            Checksum::sum((&self.peer_addr as *const IPv4Addr) as usize, mem::size_of::<IPv4Addr>()) +
                                            Checksum::sum((&proto as *const n16) as usize, mem::size_of::<n16>()) +
                                            Checksum::sum((&segment_len as *const n16) as usize, mem::size_of::<n16>()) +
                                            Checksum::sum((&tcp.header as *const TCPHeader) as usize, mem::size_of::<TCPHeader>()) +
                                            Checksum::sum(tcp.options.as_ptr() as usize, tcp.options.len()) +
                                            Checksum::sum(tcp.data.as_ptr() as usize, tcp.data.len())
                                        );
                                    }

                                    self.ip.write(&tcp.to_bytes());

                                    return true;
                                } else {
                                    return false;
                                }
                            }
                        }
                    }
                    None => return false,
                }
            },
            None => return false,
        }
    }

    /// Try to establish a server connection
    pub fn server_establish(&mut self, syn: TCP) -> bool {
        //Send SYN-ACK
        self.acknowledge += 1;
        let mut tcp = TCP {
            header: TCPHeader {
                src: n16::new(self.host_port),
                dst: n16::new(self.peer_port),
                sequence: n32::new(self.sequence),
                ack_num: n32::new(self.acknowledge),
                flags: n16::new(((mem::size_of::<TCPHeader>() << 10) & 0xF000) as u16 | TCP_SYN |
                                TCP_ACK),
                window_size: n16::new(65535),
                checksum: Checksum { data: 0 },
                urgent_pointer: n16::new(0),
            },
            options: Vec::new(),
            data: Vec::new(),
        };

        unsafe {
            let proto = n16::new(0x06);
            let segment_len =
                n16::new((mem::size_of::<TCPHeader>() + tcp.options.len() + tcp.data.len()) as u16);
            tcp.header.checksum.data =
                Checksum::compile(Checksum::sum((&IP_ADDR as *const IPv4Addr) as usize,
                                                mem::size_of::<IPv4Addr>()) +
                                  Checksum::sum((&self.peer_addr as *const IPv4Addr) as usize,
                                                mem::size_of::<IPv4Addr>()) +
                                  Checksum::sum((&proto as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&segment_len as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&tcp.header as *const TCPHeader) as usize,
                                                mem::size_of::<TCPHeader>()) +
                                  Checksum::sum(tcp.options.as_ptr() as usize, tcp.options.len()) +
                                  Checksum::sum(tcp.data.as_ptr() as usize, tcp.data.len()));
        }

        match self.ip.write(&tcp.to_bytes()) {
            Some(_) => loop { // Wait for ACK
                let mut bytes: Vec<u8> = Vec::new();
                match self.ip.read_to_end(&mut bytes) {
                    Some(_) => {
                        if let Some(segment) = TCP::from_bytes(bytes) {
                            if segment.header.dst.get() == self.host_port &&
                               segment.header.src.get() == self.peer_port {
                                if (segment.header.flags.get() & (TCP_PSH | TCP_SYN | TCP_ACK)) ==
                                   TCP_ACK {
                                    self.sequence = segment.header.ack_num.get();
                                    self.acknowledge = segment.header.sequence.get();
                                    return true;
                                } else {
                                    return false;
                                }
                            }
                        }
                    }
                    None => return false,
                }
            },
            None => return false,
        }
    }
}

impl Drop for Resource {
    fn drop(&mut self) {
        //Send FIN-ACK
        let mut tcp = TCP {
            header: TCPHeader {
                src: n16::new(self.host_port),
                dst: n16::new(self.peer_port),
                sequence: n32::new(self.sequence),
                ack_num: n32::new(self.acknowledge),
                flags: n16::new((((mem::size_of::<TCPHeader>()) << 10) & 0xF000) as u16 | TCP_FIN |
                                TCP_ACK),
                window_size: n16::new(65535),
                checksum: Checksum { data: 0 },
                urgent_pointer: n16::new(0),
            },
            options: Vec::new(),
            data: Vec::new(),
        };

        unsafe {
            let proto = n16::new(0x06);
            let segment_len =
                n16::new((mem::size_of::<TCPHeader>() + tcp.options.len() + tcp.data.len()) as u16);
            tcp.header.checksum.data =
                Checksum::compile(Checksum::sum((&IP_ADDR as *const IPv4Addr) as usize,
                                                mem::size_of::<IPv4Addr>()) +
                                  Checksum::sum((&self.peer_addr as *const IPv4Addr) as usize,
                                                mem::size_of::<IPv4Addr>()) +
                                  Checksum::sum((&proto as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&segment_len as *const n16) as usize,
                                                mem::size_of::<n16>()) +
                                  Checksum::sum((&tcp.header as *const TCPHeader) as usize,
                                                mem::size_of::<TCPHeader>()) +
                                  Checksum::sum(tcp.options.as_ptr() as usize, tcp.options.len()) +
                                  Checksum::sum(tcp.data.as_ptr() as usize, tcp.data.len()));
        }

        self.ip.write(&tcp.to_bytes());
    }
}

/// A TCP scheme
pub struct Scheme;

impl Scheme {
    pub fn new() -> Box<Scheme> {
        box Scheme
    }

    pub fn open(&mut self, url_str: &str) -> Option<Box<Resource>> {
        let url = URL::from_str(&url_str);

        if url.host().len() > 0 && url.port().len() > 0 {
            let peer_addr = IPv4Addr::from_string(&url.host());
            let peer_port = url.port().to_num() as u16;
            let host_port = (rand() % 32768 + 32768) as u16;

            if let Some(ip) = File::open(&("ip://".to_string() + &peer_addr.to_string() + "/6")) {
                let mut ret = box Resource {
                    ip: ip,
                    peer_addr: peer_addr,
                    peer_port: peer_port,
                    host_port: host_port,
                    sequence: rand() as u32,
                    acknowledge: 0,
                };

                if ret.client_establish() {
                    return Some(ret);
                }
            }
        } else if url.path().len() > 0 {
            let host_port = url.path().to_num() as u16;

            while let Some(mut ip) = File::open("ip:///6") {
                let mut bytes: Vec<u8> = Vec::new();
                match ip.read_to_end(&mut bytes) {
                    Some(_) => {
                        if let Some(segment) = TCP::from_bytes(bytes) {
                            if segment.header.dst.get() == host_port && (segment.header.flags.get() & (TCP_PSH | TCP_SYN | TCP_ACK)) == TCP_SYN {
                                if let Some(path) = ip.path() {
                                    let url = URL::from_string(&path);

                                    let peer_addr = IPv4Addr::from_string(&url.host());

                                    let mut ret = box Resource {
                                        ip: ip,
                                        peer_addr: peer_addr,
                                        peer_port: segment.header.src.get(),
                                        host_port: host_port,
                                        sequence: rand() as u32,
                                        acknowledge: segment.header.sequence.get(),
                                    };

                                    if ret.server_establish(segment) {
                                        return Some(ret);
                                    }
                                }
                            }
                        }
                    }
                    None => break,
                }
            }
        }

        None
    }
}
