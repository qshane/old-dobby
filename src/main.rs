/*
	dobby - teamspeak 3 bot built in Rust
*/

#![feature(phase)]

extern crate debug;
extern crate http;
extern crate serialize;
extern crate regex;
extern crate url;
extern crate sync;
#[phase(plugin)] extern crate regex_macros;

use http::client::RequestWriter;
use http::method::{Post,Get};
use http::headers::HeaderEnum;
use serialize::json;

use std::rand::{task_rng,Rng};
use std::sync::{Arc,Mutex};

use std::io::timer;
use std::time::Duration;
use std::collections::HashMap;
use bot::Bot;
use std::os::getenv;
use url::{Url,QUERY_ENCODE_SET};

mod connection;
mod command;
mod bot;

enum ChildDispatch {
	ChannelList(HashMap<uint,bool>),
	ClientList(Vec<HashMap<String, String>>),
	ServerInfo(HashMap<String,String>),
	ChatMessage(/*channelid: */ uint, /*invokerid: */ uint, /*invokeruid: */ String, /*message: */ String)
}

enum ParentDispatch {
	SendChatMessage(uint, String),
	GetChannelList,
	GetClientList,
	GetServerInfo,
	KickUser(uint, String),
	DeleteChannel(uint),
	Die
}

fn random_string(mut length: uint) -> String {
  let mut string: Vec<u8> = Vec::new();
  let charset = "abcdefghijklmnopqrstuvwxyz".as_bytes();

  while {(length>0)} {
  	let r = task_rng().gen_range::<uint>(0, 25);
  	string.push(charset[r]);

  	length = length-1;
  }

  String::from_utf8(string).unwrap()
}

fn watcher(cid: uint, parent: &Sender<ChildDispatch>, local: &Receiver<ParentDispatch>, error: Sender<Result<(), String>>) {
	let (err, notify, me) = Bot::new();

	spawn(proc() {
		error.send(err.recv());
	});

	let parent_2 = parent.clone();

	spawn(proc() {
		loop {
			match notify.recv_opt() {
				Ok(notification) => {
					let mut msg = String::new();
					let mut iid = 0u;
					let mut iuid = String::new();

					for arg in notification.iter_cmd() {
						match *arg {
							command::KeyValue(ref key, ref value) => {
								match key.as_slice() {
									"msg" => {
										msg = value.clone();
									},
									"invokerid" => {
										iid = from_str::<uint>(value.as_slice()).unwrap();
									},
									"invokeruid" => {
										iuid = value.clone();
									},
									_ => {

									}
								}
							},
							_ => {

							}
						}
					}

					parent_2.send(ChatMessage(cid, iid, iuid, msg));
				},
				Err(_) => {
					break;
				}
			}
		}
	});

	match me.login() {
		Ok(client_id) => {
			if me.change_name(random_string(10)) {
				me.watch_channel(client_id, cid);
			}
		},
		Err(_) => {}
	}

	loop {
		let task = local.recv();

		match task {
			Die => {
				break;
			},
			_ => {

			}
		}
	}
}

fn supervisor(parent: &Sender<ChildDispatch>, local: &Receiver<ParentDispatch>, error: Sender<Result<(), String>>) {
	let (err, notify, supervisor) = Bot::new();

	spawn(proc() {
		error.send(err.recv());
	});

	let mut local_client_id = 0u;

	match supervisor.login() {
		Ok(client_id) => {
			local_client_id = client_id;

			supervisor.change_name("Dobby".to_string());
		},
		Err(_) => {
			
		}
	}

	loop {
		let task = local.recv();
		let clientlist_lock = Arc::new(sync::mutex::Mutex::new());

		match task {
			SendChatMessage(channel, msg) => {
				if supervisor.move_to_channel(local_client_id, channel) {
					supervisor.send_chat_message(msg);
				}
			},
			GetServerInfo => {
				parent.send(ServerInfo(supervisor.server_info()));
			},
			GetChannelList => {
				parent.send(ChannelList(supervisor.channel_list()));
			},
			GetClientList => {
				let tmpbot = supervisor.clone();
				let tmpparent = parent.clone();
				let tmplock = clientlist_lock.clone();
				spawn(proc() {
					match tmplock.try_lock() {
						Some(mylock) => {
							let mut result: Vec<HashMap<String, String>> = Vec::new();

							let list = tmpbot.client_list();

							for clid in list.iter() {
								match tmpbot.get_client_info(*clid) {
									Some(ci) => {
										result.push(ci);
									},
									None => {
										return;
									}
								}
							}

							tmpparent.send(ClientList(result));
						},
						None => {

						}
					};
				});
			},
			KickUser(clid, reason) => {
				supervisor.kick_user(clid, &reason);
			},
			DeleteChannel(cid) => {
				supervisor.delete_channel(cid);
			},
			Die => {
				break;
			}
		}
	}
}

fn main() {
	let (supervisor_tx, our_rx) = channel();
	let (our_tx, supervisor_rx) = channel();

	let d_our_tx = our_tx.clone();
	let c_watcher_tx = supervisor_tx.clone();

	let server_uptime = Arc::new(Mutex::new(0u));

	spawn(proc() {
		loop {
			println!("Dobby supervisor is starting");
			let internal_our_tx = d_our_tx.clone();

			// drain the channel
			loop {
				match supervisor_rx.try_recv() {
					Ok(_) => {},
					Err(_) => {break;}
				}
			}

			let (etx, erx) = channel();

			spawn(proc() {
				println!("Supervisor encountered error: {}", erx.recv());
				internal_our_tx.send(Die);
			});

			// start the supervisor
			supervisor(&supervisor_tx, &supervisor_rx, etx);

			// since the supervisor failed, wait 10 seconds and try again
			timer::sleep(Duration::seconds(30));
		}
	});

	let another_our_tx = our_tx.clone();
	spawn(proc() {
		loop {
			timer::sleep(Duration::seconds(5));
			another_our_tx.send(GetChannelList);
			timer::sleep(Duration::seconds(1));
			another_our_tx.send(GetServerInfo);
			timer::sleep(Duration::seconds(1));
			another_our_tx.send(GetClientList);
		}
	});

	let mut currentChannelList: Arc<Mutex<HashMap<uint, (Sender<ParentDispatch>)>>> = Arc::new(Mutex::new(HashMap::new()));

	loop {
		let event = our_rx.recv();
		match event {
			ServerInfo(map) => {
				match map.find(&"virtualserver_uptime".to_string()) {
					Some(uptime) => {
						let mut v_uptime = server_uptime.lock();
						(*v_uptime) = from_str::<uint>(uptime.as_slice()).unwrap();
					},
					None => {}
				};

				spawn(proc() {
					match map.find(&"virtualserver_hostbanner_gfx_url".to_string()) {
						Some(imgurl) => {
							let url = format!("https://quibs.org/ts3_img.php?pass={}&url={}",
								getenv("TS3_PASS").unwrap(),
								url::percent_encode(command::unescape(imgurl).as_bytes(), QUERY_ENCODE_SET)
							);

							let request: RequestWriter = RequestWriter::new(Get, Url::parse(url.as_slice()).unwrap()).unwrap();

							match request.read_response() {
								_ => {}
							}
						},
						None => {}
					}
				});
			},
			ClientList(list) => {
				let copy_our_tx = our_tx.clone();
				spawn(proc() {
					// we need to kick people named Sean or quibs who aren't actually Sean or quibs
					// let's start by creating an ALGORITHM

					// iterate over all of the clients in the server
					// list is a Vec<>
					for client in list.iter() {
						// iterate over the client info until we get the nickname
						// client is a HashMap<String,String>

						let mut clid = 0u;
						let mut cid = 0u;

						let mut is_quibs = false;
						let mut is_sean = false;
						let mut is_actually_quibs = false;
						let mut is_actually_sean = false;

						for (key, value) in client.iter() {
							if key.as_slice() == "client_nickname" {
								if value.as_slice() == "quibs" {
									is_quibs = true;
								}
								if value.as_slice() == "Sean" {
									is_sean = true;
								}
							}

							if key.as_slice() == "client_database_id" {
								if value.as_slice() == "3" {
									is_actually_quibs = true;
								}

								if value.as_slice() == "6126" {
									is_actually_sean = true;
								}
							}

							if key.as_slice() == "clid" {
								clid = from_str::<uint>(value.as_slice()).unwrap();
							}

							if key.as_slice() == "cid" {
								cid = from_str::<uint>(value.as_slice()).unwrap();
							}
						}

						if is_sean && !is_actually_sean {
							copy_our_tx.send(KickUser(clid, "Imposter!".to_string()));
							println!("Kicking Sean imposter {}!", clid);
						}

						if is_quibs && !is_actually_quibs {
							copy_our_tx.send(KickUser(clid, "Imposter!".to_string()));
							println!("Kicking quibs imposter {}!", clid);
						}

						if is_actually_quibs && task_rng().gen_range::<uint>(0, 50000) == 0 {
							println!("Sending channel {} (which quibs is in) the weed thing.", cid);
							copy_our_tx.send(SendChatMessage(cid, "

╔╦╗─╔╦╗─╔╦╗─╔╦╦╦╦╦╦╗─╔╦╦╦╦╦╦╗─╔╦╦╦╦╦╗
╠╬╣─╠╬╣─╠╬╣─╠╬╬╩╩╩╩╝─╠╬╬╩╩╩╩╝─╠╬╬╩╩╬╬╗
╠╬╣─╠╬╣─╠╬╣─╠╬╬╦╦╦╦╗─╠╬╬╦╦╦╦╗─╠╬╣──╠╬╣
╠╬╣─╠╬╣─╠╬╣─╠╬╬╩╩╩╩╝─╠╬╬╩╩╩╩╝─╠╬╣──╠╬╣
╚╬╣─╠╬╣─╠╬╝─╠╬╣──────╠╬╣──────╠╬╣──╠╬╣
─╚╬╦╬╩╬╦╬╝──╠╬╬╦╦╦╦╗─╠╬╬╦╦╦╦╗─╠╬╬╦╦╬╬╝
──╚╩╝─╚╩╝───╚╩╩╩╩╩╩╝─╚╩╩╩╩╩╩╝─╚╩╩╩╩╩╝".to_string()));
						}
					}

					let encoded = json::encode(&list);

					let url = format!("https://quibs.org/ts3_clients.php?pass={}",
						getenv("TS3_PASS").unwrap()
					);

					let mut request: RequestWriter = RequestWriter::new(Post, Url::parse(url.as_slice()).unwrap()).unwrap();
					request.headers.content_length = Some(encoded.len());
					request.write(encoded.as_bytes());

					let response = match request.read_response() {
						_ => {}
					};
				});
			},
			ChannelList(map) => {
				// check for deleted channels
				{
					let mut ccl = currentChannelList.lock();
					for (channelid, tx) in ccl.iter() {
						if !map.contains_key(channelid) {
							println!("Killing bot that doesn't need to exist anymore in channel ID {}", channelid);
							tx.send(Die);
						}
					}
				}

				// check for new channels
				for (channelid, _) in map.iter() {
					let channelid = *channelid;
					//if channelid == 77 {
						let no_contains = {
							let mut ccl = currentChannelList.lock();
							!ccl.contains_key(&channelid)
						};

						if no_contains {
							println!("Starting watcher on channel {}", channelid);

							let (watcher_tx, w_our_rx) = channel();
							let (w_our_tx, watcher_rx) = channel();

							let internal_our_w_tx = w_our_tx.clone();
							
							let (etx, erx) = channel();

							spawn(proc() {
								println!("Watcher {} encountered error: {}", channelid, erx.recv());
								internal_our_w_tx.send(Die);
							});

							let our_watcher_tx = c_watcher_tx.clone();
							spawn(proc() {
								loop {
									match w_our_rx.recv_opt() {
										Ok(msg) => {
											our_watcher_tx.send(msg);
										},
										Err(_) => {
											break;
										}
									}
								}
							});

							let sub_currentChannelList = currentChannelList.clone();
							spawn(proc() {
								watcher(channelid, &watcher_tx, &watcher_rx, etx);
								
								let mut ccl = sub_currentChannelList.lock();
								ccl.remove(&channelid);
							});

							let mut ccl = currentChannelList.lock();
							ccl.insert(channelid, (w_our_tx));
						}
					//}
				}
			},
			ChatMessage(channelid, invokerid, invokeruid, message) => {
				if (invokeruid.as_slice() != "serveradmin") {
					println!("Received chat message on channel {} from {}: {}", channelid, invokerid, message);

					let re = regex!(r"^\.img (.+)$");

					let copy_our_tx = our_tx.clone();

					spawn(proc() {
						let url = format!("https://quibs.org/ts3_chat.php?pass={}&cid={}&invokerid={}&invokeruid={}&msg={}", 
							getenv("TS3_PASS").unwrap(),
							url::percent_encode(format!("{}", channelid).as_bytes(), QUERY_ENCODE_SET),
							url::percent_encode(format!("{}", invokerid).as_bytes(), QUERY_ENCODE_SET),
							url::percent_encode(command::unescape(&invokeruid).as_bytes(), QUERY_ENCODE_SET),
							url::percent_encode(command::unescape(&message).as_bytes(), QUERY_ENCODE_SET)
						);

						let request: RequestWriter = RequestWriter::new(Get, Url::parse(url.as_slice()).unwrap()).unwrap();

						match request.read_response() {
							Ok(mut response) => {
								let body = String::from_utf8(response.read_to_end().unwrap()).unwrap();

								match json::decode::<OperationList>(body.as_slice()) {
									Ok(ref res) => {
										for cmd in res.commands.iter() {
											match (*cmd.find(&"type".to_string()).unwrap()).as_slice() {
												"respond" => {
													copy_our_tx.send(SendChatMessage(channelid, (*cmd.find(&"msg".to_string()).unwrap()).clone()))
												},
												"delete_channel" => {
													copy_our_tx.send(DeleteChannel(channelid));
												},
												_ => {
													println!("unrecognized type");
												}
											}
										}
									},
									Err(_) => {
										println!("couldn't parse output");
									}
								}
							},
							Err(_) => {
								println!("couldn't fetch response");
							}
						}
					});
				}
			}
		}
	}
}

#[deriving(Decodable,Show)]
struct OperationList {
	commands: Vec<HashMap<String, String>>
}
