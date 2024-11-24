# Backend design

The main features are:

- File transfer
    - Providing a file
    - Downloading a file
- HTTP proxy
    - Provide HTTP proxy service
    - Use another node as HTTP proxy for a cost

This document outlines the different protocols used to achieve the above features and some high level implementation details.

## File transfer (Download)

![alt File Transfer](/diagrams/images/file_transfer.png)

File transfer involves a 2-step protocol, one for fetching metadata and the other for the actual download. 
Note that the 2nd step is only performed when you want to actually download a file. The protocol is detailed below:

### File Metadata Request

File metadata request is used to fetch metadata about a given file_hash. When a peer receives a metadata request, it responds with relevant information it provides the file and rejects if it doesn’t provide the file anymore.

- Success response
    - File name
    - Price
    - Size
- Failure response
    - Not a provider for the file
    - Internal error

### File Content Request

File content request is for the actual file transfer. This currently uses a protocol based on streams but can use HTTP as well. The stream based protocol use asynchronous stream based transfer.

- Client opens a stream and sends the request to the server with a request_id
- The client waits for the server to give a response to this request_id with a timeout
- The server that receives the request prepares the file, opens a stream to the client and sends it along with the request_id the client sent. Additionally, the server also sends a payment reference number.
- The client uses this to match which request it belongs to and sends the response to the appropriate caller

A HTTP based protocol would be much simpler but requires the sender to have a public IP to cover all cases.

### File Payment

The client node creates a transaction with the price of the file and then reports the transaction id to the server through a payment notification request. The server will store this and later validate that the transaction has gotten into a block. The server has to rely on the client to get to know the transaction id because we will have to jump through hoops (like using unique price or unique btc address per client) to properly match a transaction with a client without explicit notification from a client.

## HTTP proxy

![alt File Transfer](/diagrams/images/proxy_connection.png)

Proxy connection also uses a 2-step protocol, one for requesting metadata and one for the actual connection. 
Just like file metadata, the 2nd step is only performed when an actual connection is required.

### Proxy Metadata Request

Proxy metadata request is to get metadata about the proxy. If the node is not a provider, it should reject this request.

- Success Response
    - When node is a proxy provider
    - Metadata
        - IP_Addr:Port of proxy node (ex: `130.245.173.221:3000`)
        - fee rate
- Failure Response
    - Not a proxy provider
    - Internal error

### Proxy Connect Request

Proxy connect request is used to ask a provider to start accepting our proxy requests. The server should provide the client with all the information needed for the client get its requests authorized.

Response

- client_id
- auth_token
- metadata (same as metadata request)

### Proxy Payment

Payment is a 2 phase process:

- Phase 1: Client-Server consensus
- Phase 2: Payment and Transaction information exchange

### Client-Server consensus

The paying client needs to tell the server about what it thinks it owes. The server will verify and will also respond with what the server thinks the client owes. If either party feels that the other is too far apart, then they can choose to terminate the connection. Let’s see the success case for now.

In the success case:

- Client sends the data transferred in KB and fee owed based on the fee rate and payments it has already sent (may be unconfirmed)
- Server makes sure the values are not too far off and sends a payment request in the response. The payment request contains:
    - amount to send
    - recipient address
    - a payment reference
- The client verifies the server values for data transferred and fee owed

### Payment and Transaction information exchange

- After verifying, the client creates a transaction with the requested amount
- The client then sends a payment notification to the server with:
    - the transaction id
    - the payment reference given by the server
- The server notes down the transaction id and later verifies that the transaction exists, the amount is correct and that the transaction is confirmed

## Other Implementation details

### Data Storage

All data is stored in SQLite tables. We have the following tables:

- ProvidedFiles
    - To store files provided by the user
- DownloadFiles
    - Stores download history
- Payments
    - Tracks expected payments and payments made by the client
- ProxyClients
    - To store proxy client information, only used when the node is a proxy provider
    - It stores authorization information along with usage stats for each client
- ProxySessions
    - To store proxy session information, only used when the node is a proxy client. That is, when the node uses a proxy provider.
    - It stores authorization information, server information and usage stats for each session

### Network requests

We use libp2p for all communication between peers. There are 2 types of communication protocols used:

- Request-Response
    - A default request-response implementation provided in [libp2p_request_response](https://docs.rs/libp2p-request-response/latest/libp2p_request_response/) that uses substreams for communication
- Stream request
    - A custom stream based communication that involves client and server opening streams to each other for communication
    - Requests and responses are matched using request_id
    - This is implemented because we found `libp2p_request_response` is unable to handle large data transfers while streams can handle arbitrarily large data

### Peer Discovery

Peer Discovery is done through Kademlia DHT which is also run with libp2p. We use it for 2 cases:

- Find providers of a file
    - All peers that provide a file put a provider record using `PUT_PROVIDER` operation with the SHA256 hash of the file as the key
    - Each provider record will have a TTL which the provider must keep refreshing. In rust, this refreshing is done automatically.
    - Clients query the DHT for a file using the hash to get the providers of a file
- Proxy provider discovery
    - All proxy providers put a provider record using `PUT_PROVIDER` for the key `http_proxy_providers`
    - Any client that wants to know the proxy providers would query the DHT for the key `http_proxy_providers`

### Different threads

There are several responsibilities for a node and hence we have several threads each with a specific well-defined role. They are:

- HTTP Server
    - This is a local HTTP server that the desktop application will use for all interactions with the backend
- NetworkEventLoop
    - This thread listens for all network events such as:
        - Kademlia events
        - Request-Response events
        - Stream requests
        - Network commands for outgoing network requests
- RequestHandlerLoop
    - This thread handles all the requests from other peers. The network event loop is only an entry point for requests and it forwards all requests to the request handler.
    - It also handles local requests for file providing and stopping file providing, starting and stopping proxy etc.
- ProxyPaymentLoop
    - This thread handles proxy payments. It periodically checks if there are pending payments based on the data transferred since the last check and uses the 2 phase payment protocol to pay the provider
- PaymentVerificationLoop
    - This thread periodically queries unconfirmed transactions from the DB and checks if they got into a block. This is so we can track how much of the payment is actually received and confirmed.