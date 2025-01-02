# Backend Progress
Main features:

- File transfer
    - Provide a file
    - Download a file
- HTTP proxy
    - Use another peer as a HTTP proxy
    - Provide proxy service to other peers
- Wallet

All the above features are completed in the backend with APIs for configuring proxy, providing file etc. But there are edge cases within some features that are yet to be handled. This document provides information on the general project layout, some information on the implemented features and the edge cases that are yet to be implemented.

## Project Layout

For the project, we have multiple binaries:

- Relay server
    - Used as an intermediary server for nodes behind NAT to communicate with each other
    - Must be run in a public node
- Bootstrap node
    - A Kademlia node that new nodes can initially connect to
- Orca node
    - A node in our file sharing network
    - Has file sharing and proxy related behaviour
    - Also has Kademlia DHT behaviour

The code for these binaries are grouped together in their own binary crate inside the `bin` directory:

```text
├── bin
│   ├── boot_node
│   ├── orca_node
│   └── relay_server
```

### Orca_node crate

| Module | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| --- |----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| common | Contains the common utility code needed for the rest of the package. Has types, macros and util functions.                                                                                                                                                                                                                                                                                                                                                                                               |
| db | Contains the table schema and wrapper structs for accessing different tables in our database. We use rust diesel as ORM and the `db_client.rs` file has structs to interact with different tables.                                                                                                                                                                                                                                                                                                       |
| http_server | Contains the code for handling all HTTP requests and the code for starting the HTTP server. We use rocket rs as the web framework and the `endpoints` module contains the endpoints grouped under different files for different use cases (like `file_endpoints.rs` and `proxy_endpoints.rs` )                                                                                                                                                                                                           |
| network | Contains network client and event loop code for interacting with our file sharing network. We have a network event loop which listens for events from the swarm to handle libp2p events (Kademlia, Stream etc). It also listens to OrcaCommand events which are events sent from within our application but from different threads. These commands are used to interact with the network using the swarm object. Network client is a wrapper struct that uses the commands to interact with the network. |
| proxy | Contains code to set up a local proxy that either uses another node as proxy or serves as proxy. `proxy_handlers.rs`  contains the handlers `ProxyProvider` and `ProxyClient`  which both implement the `RequestHandler` trait in different ways for their respective behaviour. `proxy_payment.rs`  contains `ProxyPaymentLoop` which periodically pays the proxy provider by following the proxy payment protocol described in [HLD doc](BackendHLD.md).                                               |
| cli_handlers | Contains the CLI handler for orca_node command which has setup and start_node subcommands to setup orca node (config, dependencies, building etc) and start the node.                                                                                                                                                                                                                                                                                                                                    |

Apart from these modules, we have `request_handler.rs`  file that contains the `RequestHandlerLoop` . This handles incoming requests for files, proxy providing etc by handling events (OrcaEvent) that will be sent from different threads.

## Edge cases to be handled

There are TODOs for most cases that must be handled. I’m listing only the important ones here.

### P0

1. Failure handling for OrcaNetEvent:

   In `OrcaNetEvent` , some of the events have a response that will be sent in a one shot channel and some don’t. Add responses for all events so their failures can be handled appropriately. Currently, they are assumed infallible.

2. Balance check before download

   Add file size or total price in `FileMetadataResponse` and check if there is enough balance before starting file transfer. Either this or set up a delayed payment protocol where you pay what you have now and then mine more and pay the rest.

3. Pass connections instead of creating every time in table wrappers

   In db_client, currently all table wrappers create their own Sqlite connection. Make the structs have static methods that take a `&mut SqliteConnection` instead

4. Frequent updates about data transferred during a proxy session

   Implement more frequent updates for data transfer during a proxy session in both provider and client. We have a mechanism to prevent cheating in which if either the client or server find that the value reported by the other deviates too much from their own value, they terminate the connection. For this to work, we need to update data transfer to the database more frequently to avoid too much drift.

    - For this, we need to make sure the updates are done in a way that doesn’t fail due to the database being locked due to other concurrent updates
    - This is kind of related to the case 3 because creating new connections for every update will fail. Need to share the connection somehow to allow concurrent updates.
5. Proxy payment protocol
    1. Handling for failure cases

       We have a skeleton for different failure cases during proxy payment consensus phase. But I’ve only implemented the success case. Add implementation for the failure cases. Check `ProxyPaymentLoop` in `proxy/proxy_payment.rs` for handling cases in proxy client and `handle_payment_request` method in `RequestHandlerLoop` (`request_handler.rs`) for provider that handles the pre payment request.


### P1

- `OrcaNetConfig` has all the configuration info. Some info like relay address, bootstrap address etc are hardcoded. We already have a mechanism to use config file for certain other values. Add these also there.
- I put `handle_file_content_response`  in `utils.rs` . The method  handles the response for file content request and saves the file in the system. Move it somewhere more appropriate.
- Add support to run orca_node as a daemon with start and stop commands so it can run in the background (like how we can start and stop bitcoin-core). Use a pid_file in `~/.orcanet` where we put the config file.