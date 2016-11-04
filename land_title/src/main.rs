#![feature(type_ascription)]
#![feature(question_mark)]
#![feature(custom_derive)]
#![feature(plugin)]

#[macro_use]
extern crate rustless;
extern crate jsonway;
extern crate iron;
extern crate hyper;
extern crate valico;
extern crate env_logger;
extern crate clap;
extern crate serde;
extern crate serde_json;
extern crate time;
extern crate rand;

extern crate exonum;
extern crate blockchain_explorer;
extern crate land_title;

use std::net::SocketAddr;
use std::path::Path;
use std::thread;
use std::default::Default;

use clap::{Arg, App, SubCommand};
use rustless::json::ToJson;
use rustless::{Application, Api, Nesting, Versioning, Response, Client, ErrorResponse};
use rustless::batteries::cookie::{Cookie, CookieExt, CookieJar};
use rustless::batteries::swagger;
use valico::json_dsl;
use hyper::status::StatusCode;
use serde_json::value::from_value;

use exonum::node::{Node, Configuration, TxSender, NodeChannel};
use exonum::storage::{Database, MemoryDB, LevelDB, LevelDBOptions};
use exonum::storage::{Result as StorageResult, Error as StorageError};
use exonum::crypto::{gen_keypair, PublicKey, SecretKey, HexValue, Hash};
use exonum::messages::Message;
use exonum::config::ConfigFile;
use exonum::node::config::GenesisConfig;
use blockchain_explorer::HexField;

use land_title::{ObjectsBlockchain, ObjectTx, TxCreateOwner, TxCreateObject,
                     TxModifyObject, TxTransferObject, TxRemoveObject};
use land_title::api::{ObjectsApi, ObjectInfo};

pub type Channel<B> = TxSender<B, NodeChannel<B>>;

fn save_user(storage: &mut CookieJar, role: &str, public_key: &PublicKey, secret_key: &SecretKey) {
    let p = storage.permanent();
    let e = p.encrypted();

    e.add(Cookie::new("public_key".to_string(), public_key.to_hex()));
    e.add(Cookie::new("secret_key".to_string(), secret_key.to_hex()));
    e.add(Cookie::new("role".to_string(), role.to_string()));
}

fn load_hex_value_from_cookie<'a>(storage: &'a CookieJar, key: &str) -> StorageResult<Vec<u8>> {
    if let Some(cookie) = storage.find(key) {
        println!("{}", cookie);
        if let Ok(value) = HexValue::from_hex(cookie.value) {
            return Ok(value);
        }
    }
    Err(StorageError::new(format!("Unable to find value with given key {}", key)))
}

fn load_user(storage: &CookieJar) -> StorageResult<(String, PublicKey, SecretKey)> {
    let p = storage.permanent();
    let e = p.encrypted();

    let public_key = PublicKey::from_slice(load_hex_value_from_cookie(&e, "public_key")?.as_ref());
    let secret_key = SecretKey::from_slice(load_hex_value_from_cookie(&e, "secret_key")?.as_ref());

    let public_key = public_key.ok_or(StorageError::new("Unable to read public key"))?;
    let secret_key = secret_key.ok_or(StorageError::new("Unable to read secret key"))?;
    let role = e.find("role").ok_or(StorageError::new("Unable to read role"))?.value;
    Ok((role, public_key, secret_key))
}

fn send_tx<'a, D: Database>(tx: ObjectTx, client: Client<'a>, ch: Channel<ObjectsBlockchain<D>>)
                            -> Result<Client<'a>, ErrorResponse> {
    let tx_hash = tx.hash().to_hex();
    ch.send(tx);
    let json = &jsonway::object(|json| json.set("tx_hash", tx_hash)).unwrap();
    client.json(json)
}

fn run_node<D: Database>(blockchain: ObjectsBlockchain<D>,
                         node_cfg: Configuration,
                         port: Option<u16>) {
    if let Some(port) = port {
        let mut node = Node::new(blockchain.clone(), node_cfg);
        let channel = node.channel();

        let api_thread = thread::spawn(move || {
            let channel = channel.clone();
            let blockchain = blockchain.clone();

            let api = Api::build(move |api| {
                // Specify API version
                api.version("v1", Versioning::Path);
                api.prefix("api");

                api.error_formatter(|err, _media| {
                    if let Some(e) = err.downcast::<StorageError>() {
                        let body = format!("An internal error occured: {}", e);
                        Some(Response::from(StatusCode::InternalServerError, Box::new(body)))
                    } else {
                        None
                    }
                });

                blockchain_explorer_api(api, blockchain.clone());
                land_titles_api(api, blockchain.clone(), channel.clone());
                api.mount(swagger::create_api("docs"));
            });

            let listen_address: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
            println!("LandTitles node server started on {}", listen_address);

            let mut app = Application::new(api);

            swagger::enable(&mut app,
                            swagger::Spec {
                                info: swagger::Info {
                                    title: "LandTitles API".to_string(),
                                    description: Some("Simple API to demonstration".to_string()),
                                    contact: Some(swagger::Contact {
                                        name: "Aleksandr Marinenko".to_string(),
                                        url: Some("aleksandr.marinenko@xdev.re".to_string()),
                                        ..Default::default()
                                    }),
                                    license: Some(swagger::License {
                                        name: "Demo".to_string(),
                                        url: "http://exonum.com".to_string(),
                                    }),
                                    ..Default::default()
                                },
                                ..Default::default()
                            });

            let mut chain = iron::Chain::new(app);
            let api_key = b"abacabsasdainblabla23nx8Hasojd8";
            let cookie = ::rustless::batteries::cookie::new(api_key);
            chain.link(cookie);
            iron::Iron::new(chain).http(listen_address).unwrap();
        });

        node.run().unwrap();
        api_thread.join().unwrap();
    } else {
        Node::new(blockchain, node_cfg).run().unwrap();
    }
}

fn blockchain_explorer_api<D: Database>(api: &mut Api, b1: ObjectsBlockchain<D>) {
    blockchain_explorer::make_api::<ObjectsBlockchain<D>, ObjectTx>(api, b1);
}


fn land_titles_api<D: Database>(api: &mut Api,
                                   blockchain: ObjectsBlockchain<D>,
                                   channel: Channel<ObjectsBlockchain<D>>) {

    api.namespace("obm", move |api| {

         let ch = channel.clone();
         api.post("owners", move |endpoint| {
             endpoint.params(|params| {
                 params.req_typed("name", json_dsl::string());
             });

             endpoint.handle(move |client, params| {
                 let name = params.find("name").unwrap().as_str().unwrap();

                 let (public_key, secret_key) = gen_keypair();
                 {
                     let mut cookies = client.request.cookies();
                     save_user(&mut cookies, "owner", &public_key, &secret_key);
                 }
                 let tx = TxCreateOwner::new(&public_key, &name, &secret_key);
                 send_tx(ObjectTx::CreateOwner(tx), client, ch.clone())
             })
         });

         let ch = channel.clone();
         api.post("objects", move |endpoint| {
             endpoint.params(|params| {
                 params.req_typed("title", json_dsl::string());
                 params.req_nested("points", json_dsl::array(), |params| {
                     params.req_typed("x", json_dsl::u64());
                     params.req_typed("y", json_dsl::u64());
                 });
                 params.req_typed("owner_pub_key", json_dsl::string());
                 params.req_typed("deleted", json_dsl::boolean());
             });

             endpoint.handle(move |client, params| {
                let object_info = from_value::<ObjectInfo>(params.clone()).unwrap();
                let (role, public_key, secret_key) = {
                    let r = {
                        let cookies = client.request.cookies();
                        load_user(&cookies)
                    };
                    match r {
                        Ok((r, p, s)) => (r, p, s),
                        Err(e) => return client.error(e),
                    }
                };
                let points = object_info.points
                            .iter()
                            .cloned()
                            .map(|info| info.into())
                            .collect::<Vec<u64>>();
                let tx = TxCreateObject::new(&public_key, &object_info.title, &points, &object_info.owner_pub_key, &secret_key);
                send_tx(ObjectTx::CreateObject(tx), client, ch.clone())

             })
         });

    //     let b = blockchain.clone();
    //     api.get("distributors/:id", move |endpoint| {
    //         endpoint.params(|params| {
    //             params.req_typed("id", json_dsl::u64());
    //         });

    //         endpoint.handle(move |client, params| {
    //             let id = params.find("id").unwrap().as_u64().unwrap();

    //             let drm = DigitalRightsApi::new(b.clone());
    //             match drm.distributor_info(id as u16) {
    //                 Ok(Some(info)) => client.json(&info.to_json()),
    //                 _ => client.error(StorageError::new("Unable to get distributor")),
    //             }
    //         })
    //     });

    //     let b = blockchain.clone();
    //     api.get("owners/:id", move |endpoint| {
    //         endpoint.params(|params| {
    //             params.req_typed("id", json_dsl::u64());
    //         });

    //         endpoint.handle(move |client, params| {
    //             let id = params.find("id").unwrap().as_u64().unwrap() as u16;

    //             let drm = DigitalRightsApi::new(b.clone());
    //             match drm.owner_info(id) {
    //                 Ok(Some(info)) => client.json(&info.to_json()),
    //                 _ => client.error(StorageError::new("Unable to get owner")),
    //             }
    //         })
    //     });

    //     let ch = channel.clone();
    //     api.put("contents", move |endpoint| {
    //         endpoint.params(|params| {
    //             params.req_typed("title", json_dsl::string());
    //             params.req_typed("fingerprint", json_dsl::string());
    //             params.req_typed("additional_conditions", json_dsl::string());
    //             params.req_typed("price_per_listen", json_dsl::u64());
    //             params.req_typed("min_plays", json_dsl::u64());
    //             params.req_nested("owners", json_dsl::array(), |params| {
    //                 params.req_typed("owner_id", json_dsl::u64());
    //                 params.req_typed("share", json_dsl::u64());
    //             });
    //         });

    //         endpoint.handle(move |client, params| {
    //             let content_info = from_value::<NewContent>(params.clone()).unwrap();
    //             let (role, pub_key, sec_key) = {
    //                 let r = {
    //                     let cookies = client.request.cookies();
    //                     load_user(&cookies)
    //                 };
    //                 match r {
    //                     Ok((r, p, s)) => (r, p, s),
    //                     Err(e) => return client.error(e),
    //                 }
    //             };
    //             match role.as_ref() {
    //                 "owner" => {
    //                     let owners = content_info.owners
    //                         .iter()
    //                         .cloned()
    //                         .map(|info| info.into())
    //                         .collect::<Vec<u32>>();

    //                     let tx = TxAddContent::new(&pub_key,
    //                                                &content_info.fingerprint.0,
    //                                                &content_info.title,
    //                                                content_info.price_per_listen,
    //                                                content_info.min_plays,
    //                                                &owners,
    //                                                &content_info.additional_conditions,
    //                                                &sec_key);
    //                     send_tx(DigitalRightsTx::AddContent(tx), client, ch.clone())
    //                 }
    //                 _ => client.error(StorageError::new("Unknown role")),
    //             }
    //         })
    //     });

    //     let ch = channel.clone();
    //     let b = blockchain.clone();
    //     api.put("contracts/:fingerprint", move |endpoint| {
    //         endpoint.params(|params| {
    //             params.req_typed("fingerprint", json_dsl::string());
    //         });

    //         endpoint.handle(move |client, params| {
    //             let fingerprint = {
    //                 let r = Hash::from_hex(params.find("fingerprint").unwrap().as_str().unwrap());
    //                 match r {
    //                     Ok(f) => f,
    //                     Err(e) => return client.error(e),
    //                 }
    //             };
    //             let (role, pub_key, sec_key) = {
    //                 let r = {
    //                     let cookies = client.request.cookies();
    //                     load_user(&cookies)
    //                 };
    //                 match r {
    //                     Ok((r, p, s)) => (r, p, s),
    //                     Err(e) => return client.error(e),
    //                 }
    //             };
    //             match role.as_ref() {
    //                 "distributor" => {
    //                     let drm = DigitalRightsApi::new(b.clone());
    //                     match drm.participant_id(&pub_key) {
    //                         Ok(Some(id)) => {
    //                             let tx = TxAddContract::new(&pub_key, id, &fingerprint, &sec_key);
    //                             send_tx(DigitalRightsTx::AddContract(tx), client, ch.clone())
    //                         }
    //                         _ => client.error(StorageError::new("Unknown pub_key")),
    //                     }

    //                 }
    //                 _ => client.error(StorageError::new("Unknown role")),
    //             }
    //         })
    //     });
    });
}

fn main() {
    env_logger::init().unwrap();

    let app = App::new("Land titles manager api")
        .version(env!("CARGO_PKG_VERSION"))
        .author("Aleksandr M. <aleksandr.marinenko@xdev.re>")
        .about("Demo lt validator node")
        .arg(Arg::with_name("CONFIG")
            .short("c")
            .long("config")
            .value_name("CONFIG_PATH")
            .help("Sets a node config file")
            .required(true)
            .takes_value(true))
        .subcommand(SubCommand::with_name("generate")
            .about("Generates default configuration file")
            .version(env!("CARGO_PKG_VERSION"))
            .author("Aleksandr M. <aleksandr.marinenko@xdev.re>")
            .arg(Arg::with_name("COUNT")
                .help("Validators count")
                .required(true)
                .index(1)))
        .subcommand(SubCommand::with_name("run")
            .about("Run demo node with the given validator id")
            .version(env!("CARGO_PKG_VERSION"))
            .author("Aleksandr M. <aleksandr.marinenko@xdev.re>")
            .arg(Arg::with_name("LEVELDB_PATH")
                .short("d")
                .long("leveldb-path")
                .value_name("LEVELDB_PATH")
                .help("Use leveldb database with the given path")
                .takes_value(true))
            .arg(Arg::with_name("HTTP_PORT")
                .short("p")
                .long("port")
                .value_name("HTTP_PORT")
                .help("Run http server on given port")
                .takes_value(true))
            .arg(Arg::with_name("PEERS")
                .long("known-peers")
                .value_name("PEERS")
                .help("Comma separated list of known validator ids")
                .takes_value(true))
            .arg(Arg::with_name("VALIDATOR")
                .help("Sets a validator id")
                .required(true)
                .index(1)));

    let matches = app.get_matches();
    let path = Path::new(matches.value_of("CONFIG").unwrap());
    match matches.subcommand() {
        ("generate", Some(matches)) => {
            let count: u8 = matches.value_of("COUNT").unwrap().parse().unwrap();
            let cfg = GenesisConfig::gen(count);
            ConfigFile::save(&cfg, &path).unwrap();
            println!("The configuration was successfully written to file {:?}", path);
        }
        ("run", Some(matches)) => {
            let cfg: GenesisConfig = ConfigFile::load(path).unwrap();
            let idx: usize = matches.value_of("VALIDATOR").unwrap().parse().unwrap();
            let port: Option<u16> = matches.value_of("HTTP_PORT").map(|x| x.parse().unwrap());
            let peers = match matches.value_of("PEERS") {
                Some(string) => {
                    string.split(" ")
                        .map(|x| -> usize { x.parse().unwrap() })
                        .map(|x| cfg.validators[x].address)
                        .collect()
                }
                None => {
                    cfg.validators
                        .iter()
                        .map(|v| v.address)
                        .collect()
                }
            };
            let node_cfg = cfg.to_node_configuration(idx, peers);
            match matches.value_of("LEVELDB_PATH") {
                Some(ref db_path) => {
                    println!("Using levedb storage with path: {}", db_path);
                    let mut options = LevelDBOptions::new();
                    options.create_if_missing = true;
                    let leveldb = LevelDB::new(&Path::new(db_path), options).unwrap();

                    let blockchain = ObjectsBlockchain { db: leveldb };
                    run_node(blockchain, node_cfg, port);
                }
                None => {
                    println!("Using memorydb storage");

                    let blockchain = ObjectsBlockchain { db: MemoryDB::new() };
                    run_node(blockchain, node_cfg, port);
                }
            };
        }
        _ => {
            unreachable!("Wrong subcommand");
        }
    }
}