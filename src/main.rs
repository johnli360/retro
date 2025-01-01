use axum::{
    extract::{
        self,
        ws::{Message, Utf8Bytes, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::{header, StatusCode},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Json, Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    sync::{Arc, Mutex, RwLock},
};
use tokio::{io::AsyncReadExt, sync::broadcast};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use user::User;

mod user;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=trace", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let args: Vec<_> = std::env::args().collect();
    let app_state = if let Some(state) = args.get(1).and_then(load_state) {
        info!("read state from cmd line: {state:?}");
        state
    } else {
        Arc::new(RwLock::new(BTreeMap::new()))
    };

    let ticket_routes = Router::new()
        .route("/fetch", get(fetch_ticket))
        .route("/update", post(update_ticket))
        .route("/rm", post(rm_ticket))
        .route("/inc_vote", post(inc_vote))
        .route("/dec_vote", post(dec_vote));

    let board_routes = Router::new()
        .route("/", get(board_html))
        .route("/create", post(create_board))
        .route("/fetch", get(fetch_board))
        .route("/websocket", get(websocket_handler))
        .route("/add_ticket", post(add_ticket))
        .nest("/ticket/{ticket_id}", ticket_routes);

    let app = Router::new()
        .nest("/board/{board_id}", board_routes)
        .route("/", get(index))
        .route("/style", get(css))
        .route("/script", get(js))
        .route("/save_state", get(save_state))
        .route("/board_names", get(board_names))
        .route("/login", get(login))
        .route("/validate_login/{user_id}", get(validate_login))
        .route("/boards", get(boards))
        .with_state(app_state)
        .layer(tower::ServiceBuilder::new().layer(tower_http::trace::TraceLayer::new_for_http()));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8123").await.unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

fn load_state(path: impl AsRef<std::path::Path>) -> Option<AppStateInner> {
    let result = std::fs::File::open(path.as_ref());
    let Ok(file) = result else {
        error!(
            "failed to open state file: {:?}, err: {}",
            path.as_ref(),
            result.unwrap_err()
        );
        return None;
    };
    let reader = std::io::BufReader::new(file);
    let result = serde_json::from_reader(reader);
    let Ok(json) = result else {
        error!("Failed to parse json: {}", result.unwrap_err());
        return None;
    };
    Some(json)
}

async fn save_state(State(state): AppState) -> impl IntoResponse {
    let state = state.read().unwrap();
    let s = serde_json::to_string(&*state).unwrap();
    //Json(&*state)
    s
}

async fn board_names(State(state): AppState) -> impl IntoResponse {
    let board_names: Vec<_> = state
        .read()
        .unwrap()
        .keys()
        .map(|key| key.clone())
        .collect();
    Json(board_names).into_response()
}

pub(crate) type AppStateInner = Arc<RwLock<BTreeMap<String, Arc<BoardEntry>>>>;
type AppState = State<AppStateInner>;
type BoardId = String;
type TicketId = u32;

fn default_channel() -> broadcast::Sender<Event> {
    broadcast::channel(100).0
}

#[derive(Debug, Serialize, Deserialize)]
struct BoardEntry {
    #[serde(skip)]
    #[serde(default = "default_channel")]
    tx: broadcast::Sender<Event>,
    state: BoardState,

    #[serde(skip)]
    // We require unique usernames. This tracks which usernames have been taken.
    user_set: Mutex<HashSet<crate::user::User>>,
}

impl BoardEntry {
    fn new(name: String, columns: Vec<String>) -> BoardEntry {
        BoardEntry {
            tx: default_channel(),
            state: BoardState::new(name, columns),
            user_set: Default::default(),
        }
    }

    fn broadcast(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    async fn sync_client(&self, sender: &mut futures::stream::SplitSink<WebSocket, Message>) {
        let board = self;
        // Tell the newly connected client to fetch all tickets
        let ids: Vec<_> = {
            let tickets = board.state.tickets.lock().unwrap();
            tickets.iter().map(|(id, _)| *id).collect()
        };
        for ticket_id in ids {
            debug!("syncing ticket: {ticket_id}");
            let _ = sender.send(Event::AddedTicket(ticket_id).into()).await;
        }

        let clients: Vec<_> = {
            let tickets = board.user_set.lock().unwrap();
            tickets.iter().map(|id| id.clone()).collect()
        };
        for client in clients.iter() {
            debug!("sync client: {client}");
            let _ = sender
                .send(Event::ClientConnected(client.clone()).into())
                .await;
        }
    }
}

// Our shared state
#[derive(Debug, Serialize, Deserialize)]
#[allow(unused)]
struct BoardState {
    name: String,
    columns: Vec<String>,
    tickets: Mutex<BTreeMap<TicketId, Ticket>>,
    votes: Mutex<BTreeMap<String, Vec<TicketId>>>,
}
impl BoardState {
    fn insert_ticket(&self, ticket: Ticket) -> TicketId {
        let mut tickets = self.tickets.lock().unwrap();
        let id = if let Some((id, _)) = tickets.last_key_value() {
            *id + 1
        } else {
            0
        };
        tickets.insert(id, ticket);
        id
    }

    fn new(name: String, columns: Vec<String>) -> BoardState {
        //let user_set = Mutex::new(HashSet::new());
        let tickets = Mutex::new(BTreeMap::new());
        let board_state = BoardState {
            name,
            columns,
            tickets,
            votes: Default::default(),
        };
        board_state
    }
}

#[derive(Debug, Clone)]
enum Event {
    ClientConnected(User),
    AddedTicket(TicketId),
    UpdatedTicket(TicketId),
    P2P(String),
    ClientDisconnected(User),
    RemovedTicket(TicketId),
    IncVote(TicketId),
    DecVote(TicketId),
}
impl From<Event> for Message {
    fn from(value: Event) -> Self {
        let string = match value {
            Event::ClientConnected(id) => format!("connected: {id}"),
            Event::AddedTicket(id) => format!("add_ticket: {id}"),
            Event::P2P(s) => format!("p2p: {s}"),
            Event::ClientDisconnected(id) => format!("disconnected: {id}"),
            Event::UpdatedTicket(id) => format!("update_ticket: {id}"),
            Event::RemovedTicket(id) => format!("rm_ticket: {id}"),
            Event::IncVote(id) => format!("inc_vote: {id}"),
            Event::DecVote(id) => format!("dec_vote: {id}"),
        };
        Message::Text(string.into())
    }
}

async fn create_board(
    State(state): AppState,
    extract::Path(board_name): extract::Path<BoardId>,
    Json(cols): extract::Json<Vec<String>>,
) -> impl IntoResponse {
    info!("create_board: {cols:?}");
    let mut x = state.write().unwrap();
    if x.contains_key(&board_name) {
        return StatusCode::CONFLICT;
    }
    let state = BoardEntry::new(board_name.clone(), cols);
    x.insert(board_name, Arc::new(state));
    StatusCode::CREATED
}

async fn fetch_board(State(state): AppState, Path(board): Path<BoardId>) -> impl IntoResponse {
    if let Some(board) = state.read().unwrap().get(&board) {
        (StatusCode::OK, Json(board.state.columns.clone()))
    } else {
        (StatusCode::NOT_FOUND, Json(Vec::new()))
    }
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    Path(board): Path<BoardId>,
    State(state): AppState,
    _user: User,
) -> impl IntoResponse {
    info!("websocket_handler, board: {board}");
    let state_guard = state.read().unwrap();
    let Some(board) = state_guard.get(&board).map(|r| r.clone()) else {
        return (StatusCode::NOT_FOUND, Html("").into_response()).into_response();
    };

    ws.on_upgrade(move |socket| websocket(socket, board))
}

// This function deals with a single websocket connection, i.e., a single
// connected client / user, for which we will spawn two independent tasks (for
// receiving / sending chat messages).
async fn websocket(stream: WebSocket, board: Arc<BoardEntry>) {
    // By splitting, we can send and receive at the same time.
    let (mut sender, mut receiver) = stream.split();

    // Username gets set in the receive loop, if it's valid.
    let mut user = User::new(String::new());
    // Loop until a text message is found.
    while let Some(Ok(message)) = receiver.next().await {
        if let Message::Text(name) = message {
            // If username that is sent by client is not taken, fill username string.

            // If not empty we want to quit the loop else we want to quit function.
            if let Some(u) = check_username(&board, name.as_str()) {
                user = u;
                break;
            } else {
                // Only send our client that username is taken.
                let _ = sender
                    .send(Message::Text(Utf8Bytes::from_static(
                        "Username already taken.",
                    )))
                    .await;

                return;
            }
        }
    }

    // We subscribe *before* sending the "joined" message, so that we will also
    // display it to our client.
    let mut rx = board.tx.subscribe();

    // Now send the "joined" message to all subscribers.
    board.broadcast(Event::ClientConnected(user.clone()));

    // Sync state with newly connected client
    board.sync_client(&mut sender).await;

    // Spawn the first task that will receive broadcast messages and send text
    // messages over the websocket to our client.
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            // In any websocket error, break loop.
            if sender.send(msg.into()).await.is_err() {
                break;
            }
        }
    });

    // Clone things we want to pass (move) to the receiving task.
    let tx = board.tx.clone();
    let name = user.clone();

    // Spawn a task that takes messages from the websocket, prepends the user
    // name, and sends them to all broadcast subscribers.
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {
            // Add username before message.
            let _ = tx.send(Event::P2P(format!("{name}: {text}")));
        }
    });

    // If any one of the tasks run to completion, we abort the other.
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    };

    // Send "user left" message (similar to "joined" above).
    let _ = board.broadcast(Event::ClientDisconnected(user.clone()));

    // Remove username from map so new clients can take it again.
    board.user_set.lock().unwrap().remove(&user);
}

fn check_username(board: &BoardEntry, name: &str) -> Option<User> {
    let mut user_set = board.user_set.lock().unwrap();
    let contained = user_set.iter().find(|usr| usr.name == name);
    if contained.is_none() {
        let user = User::new(name.to_owned());
        user_set.insert(user.clone());
        return Some(user);
    }
    None
}

// Include utf-8 file at **compile** time.
async fn index() -> Html<&'static str> {
    Html(std::include_str!("../chat.html"))
}

async fn board_html(
    Path(board): Path<BoardId>,
    State(state): AppState,
    _user: User,
) -> impl IntoResponse {
    debug!("board_html: {_user:?}");
    if !state.read().unwrap().contains_key(&board) {
        return (StatusCode::NOT_FOUND, Html(String::from("no such board"))).into_response();
    }

    let mut f = tokio::fs::File::open("retro.html").await.unwrap();
    let mut content = String::new();
    f.read_to_string(&mut content).await.unwrap();

    const HEADERS: [(header::HeaderName, header::HeaderValue); 1] = [(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/html"),
    )];
    (StatusCode::OK, HEADERS, Html(content)).into_response()
}

async fn css() -> impl IntoResponse {
    let mut f = tokio::fs::File::open("style.css").await.unwrap();
    let mut content = String::new();
    f.read_to_string(&mut content).await.unwrap();
    const HEADERS: [(header::HeaderName, &str); 1] = [(header::CONTENT_TYPE, "text/css")];
    (HEADERS, content)
}

async fn js() -> impl IntoResponse {
    let mut f = tokio::fs::File::open("script.js").await.unwrap();
    let mut content = String::new();
    f.read_to_string(&mut content).await.unwrap();
    const HEADERS: [(header::HeaderName, &str); 1] = [(header::CONTENT_TYPE, "text/javascript")];
    (HEADERS, content)
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, Clone)]
struct Ticket {
    text: String,
    col_id: String,
}

//#[debug_handler]
async fn add_ticket(
    State(state): AppState,
    Path(board): Path<BoardId>,
    extract::Json(ticket): extract::Json<Ticket>,
) -> impl IntoResponse {
    debug!("adding ticket: {ticket:?}");
    let Some(board) = state.read().unwrap().get(&board).map(Arc::clone) else {
        return (StatusCode::NOT_FOUND, Html("no such board")).into_response();
    };
    let ticket_id = board.state.insert_ticket(ticket);
    let _ = board.broadcast(Event::AddedTicket(ticket_id));
    Json(ticket_id).into_response()
}

async fn fetch_ticket(
    State(state): AppState,
    Path((board, ticket_id)): Path<(BoardId, TicketId)>,
) -> impl IntoResponse {
    debug!("fetch_ticket: {ticket_id}");
    //let board = state.read().unwrap()[board].clone();
    let Some(board) = state.read().unwrap().get(&board).map(Arc::clone) else {
        return (StatusCode::NOT_FOUND, Html("no such board")).into_response();
    };

    let tickets = board.state.tickets.lock().unwrap();
    if let Some(ticket) = tickets.get(&ticket_id) {
        let votes = board.state.votes.lock().unwrap();
        let vote_count: usize = votes
            .iter()
            .map(|(_, votes)| votes.iter().filter(|tid| **tid == ticket_id).count())
            .sum();

        return Json((ticket.clone(), vote_count)).into_response();
    } else {
        warn!("no ticket with id: {ticket_id}");
    }

    (StatusCode::NOT_FOUND, Html("no such ticket")).into_response()
}

async fn update_ticket(
    State(state): AppState,
    Path((board, id)): Path<(BoardId, TicketId)>,
    extract::Json(new_ticket): extract::Json<Ticket>,
) -> StatusCode {
    debug!("update_ticket: {id} to {new_ticket:?}");
    let Some(board) = state.read().unwrap().get(&board).map(Arc::clone) else {
        return StatusCode::NOT_FOUND;
    };
    let mut tickets = board.state.tickets.lock().unwrap();
    if let Some(ticket) = tickets.get_mut(&id) {
        *ticket = new_ticket;
        let _ = board.broadcast(Event::UpdatedTicket(id));
        return StatusCode::OK;
    }

    StatusCode::NOT_FOUND
}

async fn rm_ticket(
    State(state): AppState,
    Path((board, id)): Path<(BoardId, TicketId)>,
) -> impl IntoResponse {
    debug!("rm_ticket: {id}");
    let Some(board) = state.read().unwrap().get(&board).map(Arc::clone) else {
        return Err(StatusCode::NOT_FOUND);
    };
    let mut tickets = board.state.tickets.lock().unwrap();
    if let Some(ticket) = tickets.remove(&id) {
        let _ = board.broadcast(Event::RemovedTicket(id));
        return Ok(Json(ticket.clone()));
    }

    warn!("no ticket with id: {id}");
    Err(StatusCode::NOT_FOUND)
}

const VOTE_LIMIT: usize = 6;

async fn inc_vote(
    State(state): AppState,
    Path((board, ticket_id)): Path<(BoardId, TicketId)>,
    User { name: user }: crate::user::User,
) -> impl IntoResponse {
    let state = state.read().unwrap();
    let Some(board) = state.get(&board) else {
        //Board not found
        return StatusCode::NOT_FOUND;
    };

    let mut votes = board.state.votes.lock().unwrap();
    if let Some(user_votes) = votes.get_mut(&user) {
        if user_votes.len() >= VOTE_LIMIT {
            info!("user {user} has already voted: {} times", user_votes.len());
            return StatusCode::NOT_ACCEPTABLE;
        }
        user_votes.push(ticket_id);
    } else {
        votes.insert(user, Vec::from([ticket_id]));
    }
    board.broadcast(Event::IncVote(ticket_id));

    StatusCode::OK
}

async fn dec_vote(
    State(state): AppState,
    Path((board, ticket_id)): Path<(BoardId, TicketId)>,
    User { name: ref user }: crate::user::User,
) -> impl IntoResponse {
    let state = state.read().unwrap();
    let Some(board) = state.get(&board) else {
        //Board not found
        return StatusCode::NOT_FOUND;
    };

    let mut votes = board.state.votes.lock().unwrap();
    if !votes.contains_key(user) {
        // User has no votes, nothing to remove
        return StatusCode::OK;
    }
    let user_votes = votes.get_mut(user).unwrap();
    let Some(i) = user_votes.iter().position(|tid| *tid == ticket_id) else {
        info!("User has no votes for ticket: {ticket_id}, can't dec");
        return StatusCode::NOT_ACCEPTABLE;
    };
    user_votes.remove(i as usize);
    board.broadcast(Event::DecVote(ticket_id));

    StatusCode::OK
}

async fn login() -> impl IntoResponse {
    let mut f = tokio::fs::File::open("login.html").await.unwrap();
    let mut content = String::new();
    f.read_to_string(&mut content).await.unwrap();
    Html(content)
}

async fn validate_login(Path(usr_id): Path<String>, State(state): AppState) -> impl IntoResponse {
    info!("validating login: {usr_id}");
    let mut headers = header::HeaderMap::new();
    //let Ok(session) = header::HeaderValue::from_str(&format!("SESSION={usr_id};")) else {
    let Ok(session) = header::HeaderValue::from_str(&format!("SESSION={usr_id}")) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    //TODO: doesn't work? workaround: set in login.html js
    headers.insert(header::SET_COOKIE, session);

    (headers, Redirect::to("/boards")).into_response()
}

async fn boards(user: User) -> impl IntoResponse {
    info!("/boards requested by {user:?}");
    let mut f = tokio::fs::File::open("boards.html").await.unwrap();
    let mut content = String::new();
    f.read_to_string(&mut content).await.unwrap();
    //const HEADERS: [(header::HeaderName, &str); 1] = [(header::CONTENT_TYPE, "text/css")];
    //(HEADERS, content)
    Html(content)
}
