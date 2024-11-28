use crate::kinode::process::canvas_demo::{
    Point, Request as CanvasRequest, Response as CanvasResponse,
};
use kinode_app_framework::{
    app, http, kiprintln, send_ws_update, Address, Message, Request, Response,
};
use std::collections::{HashMap, HashSet};

wit_bindgen::generate!({
    path: "target/wit",
    world: "canvas-demo-template-dot-os-v0",
    generate_unused_types: true,
    additional_derives: [serde::Serialize, serde::Deserialize, kinode_app_framework::SerdeJsonInto],
});

app!(
    "Canvas Demo",
    None,
    None,
    handle_api_call,
    handle_remote_request
);

#[derive(serde::Serialize, serde::Deserialize)]
struct State {
    our: Address,
    canvases: HashMap<String, Canvas>,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Canvas {
    users: HashSet<String>,
    points: Vec<Point>,
}

impl kinode_app_framework::State for State {
    fn new(our: Address) -> Self {
        let mut canvases = HashMap::new();
        canvases.insert(our.node().to_string(), Canvas::default());
        Self { our, canvases }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
enum Api {
    AddUser(String),
    RemoveUser(String),
    Draw((String, Point)),
    GetCanvasList,
    GetCanvas(String),
}

fn handle_api_call(
    _message: &Message,
    state: &mut State,
    call: Api,
) -> (http::server::HttpResponse, Vec<u8>) {
    let ok_response = http::server::HttpResponse::new(200 as u16);

    match call {
        Api::AddUser(user) => {
            // *we* want to add user to our canvas
            let canvas = state.canvases.get_mut(state.our.node()).unwrap();
            canvas.users.insert(user.clone());
            let Ok(invite_response) = Request::to((&user, state.our.process.clone()))
                .body(CanvasRequest::AddUser(user))
                .blob_bytes(serde_json::to_vec(&canvas).unwrap())
                .send_and_await_response(5)
                .unwrap()
            else {
                return (ok_response.set_status(500), vec![]);
            };
            match invite_response.body().try_into() {
                Ok(CanvasResponse::InviteAccepted) => (ok_response, vec![]),
                _ => (ok_response.set_status(502), vec![]),
            }
        }
        Api::RemoveUser(user) => {
            // *we* want to remove user from our canvas
            let users = &state.canvases.get(state.our.node()).unwrap().users;
            for target in users {
                Request::to((target, state.our.process.clone()))
                    .body(CanvasRequest::RemoveUser(user.clone()))
                    .send()
                    .unwrap();
            }
            state
                .canvases
                .get_mut(state.our.node())
                .unwrap()
                .users
                .remove(&user);
            (ok_response, vec![])
        }
        Api::Draw((canvas_id, point)) => {
            // *we* want to draw on a canvas
            // if it's our canvas, broadcast the draw to all users
            // otherwise, just send to the owner
            if canvas_id == state.our.node() {
                let canvas = state.canvases.get_mut(&canvas_id).unwrap();
                canvas.points.push(point.clone());
                for target in canvas.users.iter() {
                    Request::to((target, state.our.process.clone()))
                        .body(CanvasRequest::Draw((canvas_id.clone(), point.clone())))
                        .send()
                        .unwrap();
                }
            } else {
                Request::to((&canvas_id, state.our.process.clone()))
                    .body(CanvasRequest::Draw((canvas_id.clone(), point.clone())))
                    .send()
                    .unwrap();
            }
            (ok_response, vec![])
        }
        Api::GetCanvasList => (
            ok_response,
            serde_json::to_vec(&state.canvases.keys().collect::<Vec<_>>()).unwrap(),
        ),
        Api::GetCanvas(canvas_id) => (
            ok_response,
            serde_json::to_vec(&state.canvases.get(&canvas_id)).unwrap(),
        ),
    }
}

fn handle_remote_request(
    message: &Message,
    state: &mut State,
    server: &mut http::server::HttpServer,
    request: CanvasRequest,
) {
    match request {
        CanvasRequest::AddUser(user) => {
            let canvas_id = message.source().node();
            if user == state.our.node() {
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
                state.canvases.insert(canvas_id.to_string(), canvas);
                Response::new()
                    .body(CanvasResponse::InviteAccepted)
                    .send()
                    .unwrap();
            } else if let Some(canvas) = state.canvases.get_mut(canvas_id) {
                // owner adds someone else to canvas
                canvas.users.insert(user);
            }
        }
        CanvasRequest::RemoveUser(user) => {
            let canvas_id = message.source().node();
            if user == state.our.node() {
                // someone wants to remove us from their canvas
                state.canvases.remove(canvas_id);
            } else if let Some(canvas) = state.canvases.get_mut(canvas_id) {
                canvas.users.remove(&user);
            }
        }
        CanvasRequest::Draw((canvas_id, point)) => {
            let user = message.source().node();
            let Some(canvas) = state.canvases.get_mut(&canvas_id) else {
                kiprintln!("user {user} tried to draw on non-existent canvas {canvas_id}");
                return;
            };
            // validate that user is in canvas
            if !canvas.users.contains(user) {
                kiprintln!("user {user} is not in canvas {canvas_id}");
                return;
            }
            // draw point on frontend
            send_ws_update(
                &server,
                serde_json::to_vec(&(canvas_id.clone(), point.clone())).unwrap(),
            );
            if canvas_id == state.our.node() {
                // we need to push to everyone
                for target in canvas.users.iter() {
                    Request::to((target, state.our.process.clone()))
                        .body(CanvasRequest::Draw((canvas_id.clone(), point.clone())))
                        .send()
                        .unwrap();
                }
            } else {
                // add point to canvas
                canvas.points.push(point);
            }
        }
    }
}
