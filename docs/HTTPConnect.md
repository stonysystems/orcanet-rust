# HTTP CONNECT with 2 hops

Let’s assume that we’re using Firefox and have configured it to use proxy in the settings as shown below.

![image.png](/assets/images/firefox_proxy.png)

For this to work properly, we need to establish a TCP connection from Firefox directly to the destination server. This is what happens in order in the success case.

1. Firefox sends a HTTP CONNECT request to localhost proxy
2. Localhost proxy
    1. Receives CONNECT request from Firefox
    2. Creates a TCP connection directly to remote proxy
    3. Sends a HTTP CONNECT request through this connection, requesting the remote proxy to create a connection to the destination server
3. Remote proxy
    1. Receives a HTTP CONNECT request from localhost proxy
    2. Creates a TCP connection to the destination server
    3. Returns 200 OK if it succeeds
    4. Tries to upgrade the request connection to use other protocol: HTTPS in this case
4. Localhost proxy
    1. Receives response from Remote proxy
    2. If it 200 OK, then sends 200 OK to Firefox
    3. Tries to upgrade the request TCP connection between Firefox and itself to use other protocol: HTTPS in this case
5. Firefox
    1. Receives 200 OK
    2. Starts TLS Handshake with destination server through this TCP tunnel
        1. Tunnel = Firefox → Localhost proxy → Remote proxy → Destination server
    3. Once the tunnel is established, everything proceeds through the tunnel as it would through any TCP connection to the server: i.e TLS handshake, key exchange etc. The proxy simply copies the bytes from one stream to another and does not try to interpret any of these bytes.

![image.png](/assets/images/http_connect.png)