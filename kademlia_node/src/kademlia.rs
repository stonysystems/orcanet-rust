use libp2p::kad;
use libp2p::kad::store::MemoryStore;
use crate::get_key_with_ns;

pub fn process_kademlia_events(result: kad::QueryResult) {
    match result {
        kad::QueryResult::GetProviders(Ok(kad::GetProvidersOk::FoundProviders { key, providers, .. })) => {
            for peer in providers {
                println!(
                    "Peer {peer:?} provides key {:?}",
                    std::str::from_utf8(key.as_ref()).unwrap()
                );
            }
        }
        kad::QueryResult::GetProviders(Err(err)) => {
            eprintln!("Failed to get providers: {err:?}");
        }
        kad::QueryResult::GetRecord(Ok(
                                        kad::GetRecordOk::FoundRecord(kad::PeerRecord {
                                                                          record: kad::Record { key, value, .. },
                                                                          ..
                                                                      })
                                    )) => {
            println!(
                "Got record {:?} {:?}",
                std::str::from_utf8(key.as_ref()).unwrap(),
                std::str::from_utf8(&value).unwrap(),
            );
        }
        kad::QueryResult::GetRecord(Ok(_)) => {}
        kad::QueryResult::GetRecord(Err(err)) => {
            eprintln!("Failed to get record: {err:?}");
        }
        kad::QueryResult::PutRecord(Ok(kad::PutRecordOk { key })) => {
            println!(
                "Successfully put record {:?}",
                std::str::from_utf8(key.as_ref()).unwrap()
            );
        }
        kad::QueryResult::PutRecord(Err(err)) => {
            eprintln!("Failed to put record: {err:?}");
        }
        kad::QueryResult::StartProviding(Ok(kad::AddProviderOk { key })) => {
            println!(
                "Successfully put provider record {:?}",
                std::str::from_utf8(key.as_ref()).unwrap()
            );
        }
        kad::QueryResult::StartProviding(Err(err)) => {
            eprintln!("Failed to put provider record: {err:?}");
        }
        _ => {}
    }
}

pub fn handle_input_line_kad(kademlia: &mut kad::Behaviour<MemoryStore>, line: String) {
    let mut args = line.split(' ');
    let command = args.next();

    if command.is_none() {
        return;
    }

    let key = match args.next() {
        Some(key) => {
            let key_with_ns = get_key_with_ns(key);
            kad::RecordKey::new(&key_with_ns.as_str())
        }
        None => {
            eprintln!("Expected key");
            return;
        }
    };

    match command {
        Some("get") => {
            kademlia.get_record(key);
        }
        Some("get_providers") => {
            kademlia.get_providers(key);
        }
        Some("put") => {
            let value = match args.next() {
                Some(value) => value.as_bytes().to_vec(),
                None => {
                    eprintln!("Expected value");
                    return;
                }
            };
            let record = kad::Record {
                key,
                value,
                publisher: None,
                expires: None,
            };
            kademlia
                .put_record(record, kad::Quorum::One)
                .expect("Failed to store record locally.");
        }
        Some("put_provider") => {
            kademlia
                .start_providing(key)
                .expect("Failed to start providing key");
        }
        _ => {
            eprintln!("expected get, get_providers, put or put_provider");
        }
    }
}