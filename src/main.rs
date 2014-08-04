/*
	dobby - teamspeak 3 bot built in Rust
*/

#![feature(phase)]

extern crate http;
extern crate serialize;
extern crate regex;
extern crate url;
#[phase(plugin)] extern crate regex_macros;

use http::client::RequestWriter;
use http::method::Get;
use http::headers::HeaderEnum;
use serialize::json;

use std::rand::{task_rng,Rng};
use std::sync::{Arc,Mutex};
use std::io::timer;
use std::collections::HashMap;
use bot::Bot;

mod connection;
mod command;
mod bot;

enum ChildDispatch {
	ChannelList(HashMap<uint,bool>),
	ChatMessage(/*channelid: */ uint, /*invokerid: */ uint, /*invokeruid: */ String, /*message: */ String)
}

enum ParentDispatch {
	SendChatMessage(uint, String),
	GetChannelList,
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

		match task {
			SendChatMessage(channel, msg) => {
				if supervisor.move_to_channel(local_client_id, channel) {
					supervisor.send_chat_message(msg);
				}
			},
			GetChannelList => {
				parent.send(ChannelList(supervisor.channel_list()));
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
			timer::sleep(10000);
		}
	});

	let another_our_tx = our_tx.clone();
	spawn(proc() {
		loop {
			timer::sleep(5000);
			another_our_tx.send(GetChannelList);
		}
	});

	let mut currentChannelList: Arc<Mutex<HashMap<uint, (Sender<ParentDispatch>)>>> = Arc::new(Mutex::new(HashMap::new()));

	loop {
		let event = our_rx.recv();
		match event {
			ChannelList(map) => {
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
								println!("Watcher encountered error: {}", erx.recv());
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

					let re = regex!(r"^.img (.+)$");

					let copy_our_tx = our_tx.clone();

					spawn(proc() {
						match re.captures(command::unescape(&message).as_slice()) {
							Some(i) => {
								let i = i.at(1);
								let encoded = url::encode_component(i);

								let url = format!("http://ajax.googleapis.com/ajax/services/search/images?safe=off&v=1.0&q={}", encoded);

								let request: RequestWriter = RequestWriter::new(Get, from_str(url.as_slice()).unwrap()).unwrap();

								match request.read_response() {
									Ok(mut response) => {
										let body = String::from_utf8(response.read_to_end().unwrap()).unwrap();

										match json::decode::<GoogleImageResult>(body.as_slice()) {
											Ok(ref res) => {
												if res.responseData.results.len() > 0 {
													let ref first = res.responseData.results[0];

													let resultURL = first.find(&"url".to_string()).unwrap();

													let responseURL = "[URL]".to_string() + *resultURL + "[/URL]";

													copy_our_tx.send(SendChatMessage(channelid, responseURL));

													return;
												}
											},
											Err(_) => {}
										}
									},
									Err(_) => {}
								}
							},
							_ => {}
						}
						
						copy_our_tx.send(SendChatMessage(channelid, "No result!".to_string()));
					});
				}
			}
		}
	}
}

#[deriving(Decodable,Show)]
struct GoogleImageResult {
	responseData: GoogleImageResponses
}

#[deriving(Decodable,Show)]
struct GoogleImageResponses {
	results: Vec<HashMap<String, String>>
}