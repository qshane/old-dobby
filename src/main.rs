/*
	dobby - teamspeak 3 bot built in Rust
*/

#![feature(phase)]

use std::rand::{task_rng,Rng};
use std::sync::{Arc,Mutex};
use std::io::timer;
use connection::{TS3Connection};
use std::collections::HashMap;

mod connection;
mod command;

struct Bot {
	error: Sender<Result<(), String>>,
	run: Sender<(String, Sender<(Result<command::Atom, uint>, Sender<Result<(), String>>)>)>
}

impl Bot {
	pub fn send(&self, command: String, callback: |Result<command::Atom, uint>, &Bot, |result: Result<(), String>||) {
		let (tx, rx) = channel();
		self.run.send(
			(command, tx)
		);
		let (response, callback_bot) = rx.recv();

		callback(response, self, |result: Result<(), String>| {
			callback_bot.send(result);
		});
	}

	pub fn new() -> (Receiver<Result<(), String>>, Receiver<command::Atom>, Bot) {
		match TS3Connection::new(std::os::getenv("TS3_HOST").unwrap().as_slice()) {
			Ok(connection) => {
				let (notify_tx, notify_rx) = channel();
				let (error_tx, error_rx) = channel();
				let (message_tx, message_rx) = channel();
				let (run_tx, run_rx) = channel::<(String, Sender<(Result<command::Atom, uint>, Sender<Result<(), String>>)>)>();

				let writer = connection.clone();

				let error_tx_ret = error_tx.clone();

				spawn(proc() {
					error_tx.send(connection.read(notify_tx, message_tx));
				});

				let mut queue: Arc<Mutex<Vec<Sender<(Result<command::Atom, uint>, Sender<Result<(), String>>)>>>> = Arc::new(Mutex::new(Vec::new()));

				let queue1 = queue.clone();
				let queue2 = queue.clone();

				spawn(proc() {
					loop {
						match message_rx.recv_opt() {
							Ok((message, callback_result)) => {
								let mut myqueue = queue1.lock();

								match (*myqueue).shift() {
									Some(callback) => {
										callback.send((message, callback_result));
									},
									None => {
										callback_result.send(Err("Received unexpected response from the server.".to_string()));
									}
								}
							},
							Err(_) => {
								break;
							}
						}
					}
				});

				spawn(proc() {
					loop {
						match run_rx.recv_opt() {
							Ok((command, callback)) => {
								let mut myqueue = queue2.lock();

								(*myqueue).push(callback);

								match writer.write(command.as_slice()) {
									Err(e) => {
										
									},
									Ok(_) => {}
								}
							},
							Err(_) => {
								break;
							}
						}
					}
				});

				return (error_rx, notify_rx, Bot {
					run: run_tx,
					error: error_tx_ret
				})
			},
			Err(e) => {fail!("error! couldn't connect because {}", e)}
		}
	}
	fn login(&self) -> Result<uint, ()> {
		let (tx, rx) = channel();

		let mut client_id = 0u;

		self.send(format!("login serveradmin {}", std::os::getenv("TS3_PASS").unwrap()), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			if res.is_ok() {
				result(Ok(()));
				this.send("use 1".to_string(), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
					if res.is_ok() {
						//tx.send(true);
						result(Ok(()));

						this.send("whoami".to_string(), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
							match res {
								Ok(pipe) => {
									for args in pipe.iter_pipe() {
										for arg in args.iter_args() {
											match *arg {
												command::KeyValue(ref key, ref value) => {
													if key.as_slice() == "client_id" {
														client_id = from_str::<uint>(value.as_slice()).unwrap();
													}
												},
												_ => {}
											}
										}
									}

									if (client_id == 0) {
										result(Err(format!("Couldn't process whoami")))
									} else {
										tx.send(Ok(client_id));
										result(Ok(()))
									}
								},
								Err(code) => {
									result(Err(format!("Couldn't run whoami (Error code: {})", code)))
								}
							}
						})
					} else {
						result(Err(format!("Couldn't select server ID 1.")))
					}
				});
			} else {
				result(Err(format!("Supervisor couldn't log in. (Error code: {})", res.err().unwrap())));
			}
		});

		tx.send(Err(()));

		rx.recv()
	}

	fn change_name(&self, newname: String) -> bool {
		let (tx, rx) = channel();

		self.send(format!("clientupdate client_nickname={}", newname), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			if res.is_ok() {
				result(Ok(()));

				tx.send(true);
			} else {
				result(Err(format!("Couldn't change name to {}", newname)))
			}
		});

		tx.send(false);

		rx.recv()
	}

	fn move_to_channel(&self, clid: uint, cid: uint) -> bool {
		let (tx, rx) = channel();

		self.send(format!("clientmove clid={} cid={}", clid, cid), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			if res.is_ok() || (res.err().unwrap() == 770) {
				result(Ok(()));

				tx.send(true);
			} else {
				result(Err(format!("Couldn't move to channel {}", cid)))
			}
		});

		tx.send(false);

		rx.recv()
	}

	fn watch_channel(&self, clid: uint, cid: uint) -> bool {
		let (tx, rx) = channel();

		self.send(format!("servernotifyregister event=textchannel id={}", cid), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			if res.is_ok() {
				result(Ok(()));

				if self.move_to_channel(clid, cid) {
					tx.send(true);
				}
			} else {
				result(Err(format!("Couldn't listen to channel chat for channel {}", cid)))
			}
		});

		tx.send(false);

		rx.recv()
	}

	fn send_chat_message(&self, msg: String) {
		self.send(format!("sendtextmessage targetmode=2 target=1 msg={}", command::escape(&msg)), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			if res.is_ok() {
				result(Ok(()));
			} else {
				result(Err(format!("Couldn't send message to channel: {}", msg)));
			}
		})
	}

	fn channel_list(&self) -> HashMap<uint,bool> {
		let mut ret = HashMap::new();

		self.send("channellist".to_string(), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			match res {
				Ok(pipe) => {
					result(Ok(()));
					for args in pipe.iter_pipe() {
						for arg in args.iter_args() {
							match *arg {
								command::KeyValue(ref key, ref value) => {
									if key.as_slice() == "cid" {
										ret.insert(from_str::<uint>(value.as_slice()).unwrap(), true);
									}
								},
								_ => {

								}
							}
						}
					}
				},
				Err(code) => {
					result(Err(format!("Couldn't list channels (Error code: {})", code)))
				}
			}
		});

		ret
	}
}

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
					if channelid == 77 || channelid == 75 {
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
					}
				}
			},
			ChatMessage(channelid, invokerid, invokeruid, message) => {
				if (invokeruid.as_slice() != "serveradmin") {
					println!("Received chat message on channel {} from {}: {}", channelid, invokerid, message);
					our_tx.send(SendChatMessage(channelid, "You suck!".to_string()));
				}
			}
		}
	}
}