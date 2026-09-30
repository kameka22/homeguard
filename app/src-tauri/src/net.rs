/// The IP the OS would use to reach the outside world — a reasonable
/// stand-in for "this Mac's LAN address" without pulling in a whole
/// interface-enumeration dependency. No packet is actually sent: `connect`
/// on a UDP socket only asks the kernel to pick a route.
pub fn local_lan_ip() -> std::io::Result<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
    socket.connect("8.8.8.8:80")?;
    Ok(socket.local_addr()?.ip())
}
