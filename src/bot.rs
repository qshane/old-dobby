extern crate time;

use command;
use connection::TS3Connection;
use std::collections::HashMap;
use std::sync::{Arc,Mutex};
use std::os::getenv;
use std::time::Duration;
use std::io::timer;

#[deriving(Clone)]
pub struct Bot {
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
		match TS3Connection::new(getenv("TS3_HOST").unwrap().as_slice()) {
			Ok(connection) => {
				let (notify_tx, notify_rx) = channel();
				let (error_tx, error_rx) = channel();
				let (message_tx, message_rx) = channel();
				let (run_tx, run_rx) = channel::<(String, Sender<(Result<command::Atom, uint>, Sender<Result<(), String>>)>)>();

				let writer = connection.clone();
				let timeoutchecker = connection.clone();

				let error_tx_ret = error_tx.clone();

				spawn(proc() {
					error_tx.send(connection.read(notify_tx, message_tx));
					connection.close();
				});

				let mut queue: Arc<Mutex<Vec<Sender<(Result<command::Atom, uint>, Sender<Result<(), String>>)>>>> = Arc::new(Mutex::new(Vec::new()));

				let queue1 = queue.clone();
				let queue2 = queue.clone();

				spawn(proc() {
					loop {
						match message_rx.recv_opt() {
							Ok((message, callback_result)) => {
								let mut myqueue = queue1.lock();

								match (*myqueue).remove(0) {
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
				
				let t_run_tx = run_tx.clone();

				spawn(proc() {
					loop {
						timer::sleep(Duration::seconds(5));
						let lastmsg = timeoutchecker.last_msg();
						if (lastmsg.sec < (time::get_time().sec - 10)) {
							timeoutchecker.close();
						} else {
							// send a ping message of some sort
							let (my_response_tx,my_response_rx) = channel();
							t_run_tx.send(
								("whoami".to_string(), my_response_tx)
							);
							match my_response_rx.recv_opt() {
								Ok((result, responder)) => {
									responder.send(Ok(()));
								}
								_ => {}
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
									Err(e) => {},
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
	pub fn login(&self) -> Result<uint, ()> {
		let (tx, rx) = channel();

		let mut client_id = 0u;

		self.send(format!("login serveradmin {}", getenv("TS3_PASS").unwrap()), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
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

	pub fn change_name(&self, newname: String) -> bool {
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

	pub fn delete_channel(&self, cid: uint) {
		if cid == 1 {
			return;
		}

		self.send(format!("channeldelete cid={} force=1", cid), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			if res.is_ok() {
				result(Ok(()));
			} else {
				result(Err(format!("Couldn't delete channel {}", cid)));
			}
		})
	}

	pub fn move_to_channel(&self, clid: uint, cid: uint) -> bool {
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

	pub fn watch_channel(&self, clid: uint, cid: uint) -> bool {
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

	pub fn kick_user(&self, clid: uint, reason: &String) {
		self.send(format!("clientkick clid={} reasonid=5 reasonmsg={}", clid, command::escape(reason)), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			if res.is_ok() {
				result(Ok(()));
			} else {
				result(Err(format!("Couldn't kick user.")));
			}
		});
	}

	pub fn send_chat_message(&self, msg: String) {
		self.send(format!("sendtextmessage targetmode=2 target=1 msg={}", command::escape(&msg)), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			if res.is_ok() {
				result(Ok(()));
			} else {
				result(Err(format!("Couldn't send message to channel: {}", msg)));
			}
		})
	}

	pub fn server_info(&self) -> HashMap<String,String> {
		let mut ret = HashMap::new();

		self.send("serverinfo".to_string(), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			match res {
				Ok(pipe) => {
					result(Ok(()));

					for args in pipe.iter_pipe() {
						for arg in args.iter_args() {
							match *arg {
								command::KeyValue(ref key, ref value) => {
									ret.insert(key.clone(), value.clone());
								},
								_ => {

								}
							}
						}
					}
				},
				Err(code) => {
					result(Err(format!("Couldn't get server info (Error code: {})", code)))
				}
			}
		});

		return ret;
	}

	pub fn client_list(&self) -> Vec<uint> {
		let mut ret = Vec::new();

		self.send("clientlist".to_string(), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			match res {
				Ok(pipe) => {
					result(Ok(()));

					for args in pipe.iter_pipe() {
						for arg in args.iter_args() {
							match *arg {
								command::KeyValue(ref key, ref value) => {
									if key.as_slice() == "clid" {
										ret.push(from_str::<uint>(value.as_slice()).unwrap());
									}
								},
								_ => {}
							}
						}
					}
				},
				Err(code) => {
					result(Err(format!("Couldn't get client info (Error code: {})", code)))
				}
			}
		});

		ret
	}

	pub fn get_client_info(&self, clid: uint) -> Option<HashMap<String, String>> {
		let mut ret = HashMap::new();

		self.send(format!("clientinfo clid={}", clid), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			match res {
				Ok(pipe) => {
					result(Ok(()));

					for args in pipe.iter_pipe() {
						for arg in args.iter_args() {
							match *arg {
								command::KeyValue(ref key, ref value) => {
									ret.insert(key.clone(), value.clone());
								},
								_ => {

								}
							}
						}
					}
				},
				Err(code) => {
					println!("Failed to get client info (probably client left before we made request).");
					result(Ok(()));
					//result(Err(format!("Couldn't get client detailed info (Error code: {})", code)))
				}
			}
		});

		if (ret.len() == 0) {
			None
		}
		else {
			ret.insert("clid".to_string(), format!("{}", clid));
			Some(ret)
		}
	}

	pub fn channel_list(&self) -> HashMap<uint,String> {
		let mut ret = HashMap::new();

		self.send("channellist".to_string(), |res: Result<command::Atom, uint>, this: &Bot, result: |Result<(), String>|| {
			match res {
				Ok(pipe) => {
					result(Ok(()));
					for args in pipe.iter_pipe() {
						let mut clid: Option<uint> = None;
						let mut name: Option<String> = None;

						for arg in args.iter_args() {
							match *arg {
								command::KeyValue(ref key, ref value) => {
									if key.as_slice() == "cid" {
										clid = Some(from_str::<uint>(value.as_slice()).unwrap());
									} else if key.as_slice() == "channel_name" {
										name = Some(value.clone());
									}
								},
								_ => {

								}
							}
						}

						if clid.is_some() && name.is_some() {
							ret.insert(clid.unwrap(), name.unwrap());
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