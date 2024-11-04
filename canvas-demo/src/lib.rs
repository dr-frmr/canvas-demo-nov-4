use crate::kinode::process::canvas_demo::{
    Point, Request as CanvasRequest, Response as CanvasResponse,
};
use kinode_process_lib::{
    await_message, call_init, http, println, Address, LazyLoadBlob, Message, Request, Response,
};
use std::collections::{HashMap, HashSet};

wit_bindgen::generate!({
    path: "target/wit",
    world: "canvas-demo-template-dot-os-v0",
    generate_unused_types: true,
    additional_derives: [serde::Serialize, serde::Deserialize, process_macros::SerdeJsonInto],
});

type State = HashMap<String, Canvas>;

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Canvas {
    users: HashSet<String>,
    points: Vec<Point>,
}

#[derive(serde::Serialize, serde::Deserialize)]
enum ApiCall {
    AddUser(String),
    RemoveUser(String),
    Draw((String, Point)),
    GetCanvasList,
    GetCanvas(String),
}

call_init!(init);
fn init(our: Address) {
    // serve frontend UI to user
    let mut server = http::server::HttpServer::new(5);

    server
        .serve_ui(
            &our,
            "ui",
            vec!["/"],
            http::server::HttpBindingConfig::default(),
        )
        .expect("failed to serve UI");

    server
        .bind_http_path("/api", http::server::HttpBindingConfig::default())
        .expect("failed to serve API path");

    server
        .bind_ws_path("/updates", http::server::WsBindingConfig::default())
        .expect("failed to bind WS path");

    let mut state = State::default();

    // create a canvas for ourselves
    let mut our_canvas = Canvas::default();
    our_canvas.users.insert(our.node().to_string());
    state.insert(our.node().to_string(), our_canvas);

    loop {
        match await_message() {
            Err(send_error) => println!("got SendError: {send_error}"),
            Ok(ref message) => handle_message(&our, &mut state, message, &mut server),
        }
    }
}

fn handle_message(
    our: &Address,
    state: &mut State,
    message: &Message,
    server: &mut http::server::HttpServer,
) {
    if message.is_local(our) {
        // handle local messages
        if message.source().process == "http_server:distro:sys" {
            handle_http_request(our, message, state, server);
        }
    } else {
        // handle remote messages
        handle_remote_request(our, message, state, server);
    }
}

fn handle_http_request(
    our: &Address,
    message: &Message,
    state: &mut State,
    server: &mut http::server::HttpServer,
) {
    let http_request = serde_json::from_slice::<http::server::HttpServerRequest>(&message.body())
        .expect("failed to parse HTTP request");

    server.handle_request(
        http_request,
        |_incoming| {
            let response = http::server::HttpResponse::new(200 as u16);
            let mut blob: Option<LazyLoadBlob> = None;

            let Some(Ok(call)) = message
                .blob()
                .map(|blob| serde_json::from_slice::<ApiCall>(blob.bytes()))
            else {
                return (response.set_status(400), blob);
            };

            match call {
                ApiCall::AddUser(user) => {
                    // *we* want to add user to our canvas
                    let canvas = state.get_mut(our.node()).unwrap();
                    canvas.users.insert(user.clone());
                    let Ok(invite_response) = Request::to((&user, our.process.clone()))
                        .body(CanvasRequest::AddUser(user))
                        .blob_bytes(serde_json::to_vec(&canvas).unwrap())
                        .send_and_await_response(5)
                        .unwrap()
                    else {
                        return (response.set_status(500), blob);
                    };
                    match invite_response.body().try_into() {
                        Ok(CanvasResponse::InviteAccepted) => (),
                        _ => return (response.set_status(502), blob),
                    }
                }
                ApiCall::RemoveUser(user) => {
                    // *we* want to remove user from our canvas
                    let users = &state.get(our.node()).unwrap().users;

                    for target in users {
                        Request::to((target, our.process.clone()))
                            .body(CanvasRequest::RemoveUser(user.clone()))
                            .send()
                            .unwrap();
                    }

                    state.get_mut(our.node()).unwrap().users.remove(&user);
                }
                ApiCall::Draw((canvas_id, point)) => {
                    // *we* want to draw on a canvas
                    // if it's our canvas, broadcast the draw to all users
                    // otherwise, just send to the owner
                    if canvas_id == our.node() {
                        let canvas = state.get_mut(&canvas_id).unwrap();
                        canvas.points.push(point.clone());
                        for target in canvas.users.iter() {
                            Request::to((target, our.process.clone()))
                                .body(CanvasRequest::Draw((canvas_id.clone(), point.clone())))
                                .send()
                                .unwrap();
                        }
                    } else {
                        Request::to((&canvas_id, our.process.clone()))
                            .body(CanvasRequest::Draw((canvas_id.clone(), point.clone())))
                            .send()
                            .unwrap();
                    }
                }
                ApiCall::GetCanvasList => {
                    blob = Some(LazyLoadBlob::new(
                        Some("application/json"),
                        serde_json::to_vec(&state.keys().collect::<Vec<_>>()).unwrap(),
                    ));
                }
                ApiCall::GetCanvas(canvas_id) => {
                    blob = Some(LazyLoadBlob::new(
                        Some("application/json"),
                        serde_json::to_vec(&state.get(&canvas_id)).unwrap(),
                    ));
                }
            }

            (response, blob)
        },
        |_, _, _| {
            // skip incoming ws requests
        },
    );
}

fn handle_remote_request(
    our: &Address,
    message: &Message,
    state: &mut State,
    server: &mut http::server::HttpServer,
) {
    match message.body().try_into() {
        Ok(CanvasRequest::AddUser(user)) => {
            let canvas_id = message.source().node();
            println!("{user} got added to canvas {canvas_id}");
            if user == our.node() {
                // someone wants to add us to their canvas
                // let's automatically accept for now
                let Ok(canvas) =
                    serde_json::from_slice::<Canvas>(message.blob().unwrap_or_default().bytes())
                else {
                    Response::new()
                        .body(CanvasResponse::InviteRejected)
                        .send()
                        .unwrap();
                    return;
                };
                state.insert(canvas_id.to_string(), canvas);
                Response::new()
                    .body(CanvasResponse::InviteAccepted)
                    .send()
                    .unwrap();
            } else if let Some(canvas) = state.get_mut(canvas_id) {
                // owner adds someone else to canvas
                canvas.users.insert(user);
            }
        }
        Ok(CanvasRequest::RemoveUser(user)) => {
            let canvas_id = message.source().node();
            println!("{user} got removed from canvas {canvas_id}");
            if user == our.node() {
                // someone wants to remove us from their canvas
                state.remove(canvas_id);
            } else if let Some(canvas) = state.get_mut(canvas_id) {
                canvas.users.remove(&user);
            }
        }
        Ok(CanvasRequest::Draw((canvas_id, point))) => {
            let user = message.source().node();
            println!("{user} drew {point:?} on canvas {canvas_id}");

            let Some(canvas) = state.get_mut(&canvas_id) else {
                println!("user {user} tried to draw on non-existent canvas {canvas_id}");
                return;
            };

            // validate that user is in canvas
            if !canvas.users.contains(user) {
                println!("user {user} is not in canvas {canvas_id}");
                return;
            }

            server.ws_push_all_channels(
                "/updates",
                http::server::WsMessageType::Text,
                LazyLoadBlob::new(
                    Some("application/json"),
                    serde_json::to_vec(&(canvas_id.clone(), point.clone())).unwrap(),
                ),
            );

            if canvas_id == our.node() {
                // we need to push to everyone
                for target in canvas.users.iter() {
                    Request::to((target, our.process.clone()))
                        .body(CanvasRequest::Draw((canvas_id.clone(), point.clone())))
                        .send()
                        .unwrap();
                }
            } else {
                // add point to canvas
                canvas.points.push(point);
            }
        }
        Err(e) => println!("failed to parse request: {e}"),
    }
}
