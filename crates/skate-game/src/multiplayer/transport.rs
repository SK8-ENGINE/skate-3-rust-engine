use skate_net::directory::{Command as LobbyCommand, Response};
#[cfg(feature = "steam")]
use skate_net::directory::{self, Event, Request};
use std::{
    io,
    net::{SocketAddr, UdpSocket},
};
#[cfg(feature = "steam")]
use std::time::{Duration, Instant};
#[cfg(feature = "steam")]
use std::process::{Child, Command, Stdio};

/// Platform adapters move opaque bounded datagrams; session/actors live above this.
pub(super) trait Transport: Send + Sync {
    fn send(&mut self, peer: u64, data: &[u8]) -> io::Result<()>;
    fn receive(&mut self) -> io::Result<Vec<(u64, Vec<u8>)>>;
    fn status(&self) -> String;
    fn command(&mut self, _command: LobbyCommand) -> Result<(), String> {
        Err("Leave local multiplayer before browsing Steam lobbies".into())
    }
    fn events(&mut self) -> Vec<Response> {
        vec![]
    }
    fn loopback(&self) -> bool {
        false
    }
    fn metrics(&self) -> String {
        String::new()
    }
    fn congested(&self) -> bool {
        false
    }
}
pub(super) fn endpoint(addr: SocketAddr) -> io::Result<u64> {
    match addr {
        SocketAddr::V4(a) => Ok(((u32::from(*a.ip()) as u64) << 16) | a.port() as u64),
        _ => Err(io::Error::other("Direct testing currently requires IPv4")),
    }
}
pub(super) struct Direct {
    socket: UdpSocket,
}
impl Direct {
    pub fn new(bind: SocketAddr) -> io::Result<Self> {
        endpoint(bind)?;
        let socket = UdpSocket::bind(bind)?;
        skate_net::socket::configure(&socket)?;
        Ok(Self { socket })
    }
}
impl Transport for Direct {
    fn loopback(&self) -> bool {
        self.socket.local_addr().is_ok_and(|a| a.ip().is_loopback())
    }
    fn send(&mut self, peer: u64, data: &[u8]) -> io::Result<()> {
        let address =
            std::net::SocketAddrV4::new(std::net::Ipv4Addr::from((peer >> 16) as u32), peer as u16);
        self.socket.send_to(data, address).map(|_| ())
    }
    fn receive(&mut self) -> io::Result<Vec<(u64, Vec<u8>)>> {
        let mut output = Vec::new();
        let mut buffer = [0; 1500];
        for _ in 0..4096 {
            match self.socket.recv_from(&mut buffer) {
                Ok((n, from)) if n <= skate_net::packed::MTU => {
                    output.push((endpoint(from)?, buffer[..n].to_vec()))
                }
                Ok(_) => (),
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::ConnectionReset
                            | io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    break;
                }
                Err(e) => return Err(e),
            }
        }
        Ok(output)
    }
    fn status(&self) -> String {
        "Direct connection (Steam not required)".into()
    }
}
#[cfg(feature = "steam")]
pub(super) struct Steam {
    socket: UdpSocket,
    child: Child,
    peer: Option<SocketAddr>,
    cookie: String,
    status: String,
    last_ping: Instant,
    started: Instant,
    metrics: String,
    congested: bool,
    events: Vec<Response>,
    pending: Option<(Request, Instant)>,
    last_command: Instant,
    request_id: u64,
}
#[cfg(feature = "steam")]
impl Steam {
    pub fn new(peer: u64, session: u64) -> Result<Self, String> {
        let socket = UdpSocket::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
        skate_net::socket::configure(&socket).map_err(|e| e.to_string())?;
        let dir = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .unwrap()
            .to_path_buf();
        let relay = dir.join("steam-relay");
        let helper = relay.join(skate_platform::exe::exe_name("skate-steam-relay"));
        if !helper.is_file() || !relay.join(skate_platform::exe::steam_api_library()).is_file() {
            return Err(
                "Steam relay files missing; solo and direct multiplayer remain available".into(),
            );
        }
        let cookie = format!("{:016x}", super::unique());
        let mut command = Command::new(helper);
        command
            .args([
                socket.local_addr().map_err(|e| e.to_string())?.to_string(),
                peer.to_string(),
                session.to_string(),
                cookie.clone(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        skate_platform::process::detached(&mut command);
        let child = command
            .spawn()
            .map_err(|e| format!("Could not start Steam relay: {e}"))?;
        Ok(Self {
            socket,
            child,
            peer: None,
            cookie,
            status: "Checking Steam; open Steam and sign in if needed".into(),
            last_ping: Instant::now(),
            started: Instant::now(),
            metrics: String::new(),
            congested: false,
            events: vec![],
            pending: None,
            last_command: Instant::now() - Duration::from_secs(1),
            request_id: 0,
        })
    }
}
#[cfg(feature = "steam")]
impl Drop for Steam {
    fn drop(&mut self) {
        if let Some(peer) = self.peer {
            let _ = self
                .socket
                .send_to(format!("QUIT {}", self.cookie).as_bytes(), peer);
            for _ in 0..10 {
                if self.child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[cfg(feature = "steam")]
impl Transport for Steam {
    fn command(&mut self, command: LobbyCommand) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("Steam request in progress; please wait".into());
        }
        self.request_id += 1;
        self.pending = Some((
            Request {
                id: self.request_id,
                command,
            },
            Instant::now(),
        ));
        self.last_command = Instant::now() - Duration::from_secs(1);
        Ok(())
    }
    fn events(&mut self) -> Vec<Response> {
        std::mem::take(&mut self.events)
    }
    fn send(&mut self, target: u64, data: &[u8]) -> io::Result<()> {
        let peer = self
            .peer
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "Relay starting"))?;
        let mut packet = u64::from_str_radix(&self.cookie, 16)
            .unwrap()
            .to_le_bytes()
            .to_vec();
        packet.extend(target.to_le_bytes());
        packet.extend(data);
        self.socket.send_to(&packet, peer).map(|_| ())
    }
    fn receive(&mut self) -> io::Result<Vec<(u64, Vec<u8>)>> {
        let mut packets = vec![];
        let mut buffer = [0; 1500];
        let prefix = format!("SK8RELAY {} ", self.cookie);
        for _ in 0..4096 {
            match self.socket.recv_from(&mut buffer) {
                Ok((n, from)) => {
                    if !from.ip().is_loopback() || self.peer.is_some_and(|p| p != from) {
                        continue;
                    }
                    if let Ok(text) = std::str::from_utf8(&buffer[..n]) {
                        if let Some(status) = text.strip_prefix(&prefix) {
                            self.peer = Some(from);
                            if let Some(json) = status.strip_prefix("EVENT ") {
                                if let Some(response) = directory::response(json) {
                                    if response.request == 0
                                        || self
                                            .pending
                                            .as_ref()
                                            .is_some_and(|(r, _)| r.id == response.request)
                                    {
                                        if response.request != 0 {
                                            self.pending = None;
                                        }
                                        self.events.push(response);
                                    }
                                }
                            } else if let Some(stats) = status.strip_prefix("STATS ") {
                                self.congested = stats.split_whitespace().next() == Some("1");
                                self.metrics = stats.split_once(' ').map_or("", |(_, s)| s).into();
                            } else {
                                self.status = status.into();
                            }
                            continue;
                        }
                    }
                    if self.peer == Some(from)
                        && n >= 16
                        && n <= skate_net::packed::MTU + 16
                        && u64::from_le_bytes(buffer[..8].try_into().unwrap())
                            == u64::from_str_radix(&self.cookie, 16).unwrap()
                    {
                        packets.push((
                            u64::from_le_bytes(buffer[8..16].try_into().unwrap()),
                            buffer[16..n].to_vec(),
                        ));
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        if let Some((request, since)) = &self.pending {
            if since.elapsed() > Duration::from_secs(18) {
                self.events.push(Response {
                    request: request.id,
                    event: Event::Error(
                        "Steam request timed out. Open Steam and sign in, then retry.".into(),
                    ),
                });
                self.pending = None;
            } else if self.last_command.elapsed() > Duration::from_millis(500) {
                if let Some(peer) = self.peer {
                    let _ = self.socket.send_to(
                        format!("CMD {} {}", self.cookie, directory::encode(request)).as_bytes(),
                        peer,
                    );
                    self.last_command = Instant::now();
                }
            }
        }
        if self.last_ping.elapsed() > Duration::from_secs(1) {
            if let Some(p) = self.peer {
                let _ = self
                    .socket
                    .send_to(format!("PING {}", self.cookie).as_bytes(), p);
            }
            self.last_ping = Instant::now();
        }
        if self.child.try_wait()?.is_some() && !self.status.starts_with("ERROR") {
            self.status = "Steam relay stopped. Open Steam, sign in, and retry.".into();
        }
        if self.peer.is_none() && self.started.elapsed() > Duration::from_secs(10) {
            self.status = "Steam did not respond. Open Steam, sign in, and retry.".into();
        }
        Ok(packets)
    }
    fn status(&self) -> String {
        self.status.clone()
    }
    fn metrics(&self) -> String {
        self.metrics.clone()
    }
    fn congested(&self) -> bool {
        self.congested
    }
}

/// Steam-less builds keep direct UDP multiplayer and report Steam as absent.
#[cfg(not(feature = "steam"))]
pub(super) struct Steam;
#[cfg(not(feature = "steam"))]
impl Steam {
    pub fn new(_peer: u64, _session: u64) -> Result<Self, String> {
        Err("Steam multiplayer is not included in this build; direct multiplayer remains available".into())
    }
}
#[cfg(not(feature = "steam"))]
impl Transport for Steam {
    fn send(&mut self, _peer: u64, _data: &[u8]) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "Steam not included in this build"))
    }
    fn receive(&mut self) -> io::Result<Vec<(u64, Vec<u8>)>> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "Steam not included in this build"))
    }
    fn status(&self) -> String {
        "Steam not included in this build".into()
    }
}
