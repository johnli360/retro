main().catch(err => console.log("main err: " + err));
async function main() {

//const board_id = parseInt(document.cookie.split('board_id=')[1]);
const board_id = window.location.href.split("board/")[1];
const title = document.querySelector("#board_id");
title.textContent = board_id;
console.log("board_id: " + board_id);

const username_input = document.querySelector("#username_input");
const username_display = document.querySelector("#username_display");
const join_btn = document.querySelector("#join");
const peers = document.querySelector("#peers");

const containerDiv = document.getElementById('cols');
const columnIds = await fetch_board();

console.log("columnIds: " + columnIds);

const client_set = new Set();

const sel_class = "ticket_selected";
let selected_tickets = new Array();

document.addEventListener('keydown', function(event) {
    if (event.key === 'Enter') {
        console.log('GLobal Key pressed:', event.key);
        merge_tickets();
    }
});

function add_selected(ticket_id) {
    selected_tickets[selected_tickets.length] = ticket_id;
    console.log("add_selected: " + selected_tickets);
}

function rm_selected(ticket_id) {
    selected_tickets = selected_tickets.filter(tid => tid != ticket_id);
    console.log("rm_selected: " + selected_tickets);
}

function merge_tickets() {
    console.log("merge_tickets selected: " + selected_tickets);
    if (selected_tickets.length == 0) {
        return;
    }

    const root = document.getElementById(selected_tickets[0]);
    const root_p = root.querySelector('p');

    for (let id of selected_tickets.splice(1,selected_tickets.length)) {
        const ticket = document.getElementById(id);
        const content = ticket.querySelector('p');
        root_p.textContent += "\n\n";
        root_p.textContent += content.textContent;
        post_rm_ticket(ticket_id_to_int(id));
    }
    const col_id = root.parentNode.id.split("tickets_")[1];
    post_update_ticket(ticket_id_to_int(selected_tickets[0]),
        col_id, root_p.textContent);

    for (let id of selected_tickets.splice(0, selected_tickets.length)) {
        const ticket = document.getElementById(id);
        ticket.classList.remove(sel_class);
    }
    selected_tickets = new Array();
}

function ticket_id_to_int(ticket_id) {
    return parseInt(ticket_id.split("_")[1]);
}

function update_peers() {
    console.log("updating peers: " + client_set);
    const peers = document.getElementById('peers');
    peers.textContent = "";
    for (let client of client_set) {
        console.log("updating client: " + client);
        peers.textContent += client;
        peers.textContent += ' ';
    }
}

async function initialise() {
    for (let i = 0; i < columnIds.length; i++) {
        var col = document.createElement('div');
        const name = columnIds[i];

        {
            const header = document.createElement('div');
            const content = document.createElement('h3');
            header.appendChild(content);
            header.classList.add('col_header');
            header.id = "col_header_" + name;
            content.textContent = name;
            col.appendChild(header);
        }

        {
            const tickets = document.createElement('div');
            tickets.id = "tickets_" + name;
            tickets.classList.add('tickets');
            col.appendChild(tickets);
        }

        {
            //Add the columns textarea for submitting tickets
            const add_ticket_textarea = document.createElement('textarea');
            add_ticket_textarea.id = "textarea_" + name;
            add_ticket_textarea.classList.add('add_ticket_textarea');
            col.appendChild(add_ticket_textarea);
            update_height();

            function update_height() {
                add_ticket_textarea.rows = 1;
                while (add_ticket_textarea.clientHeight
                        < add_ticket_textarea.scrollHeight) {
                    add_ticket_textarea.rows += 1;
                }
            }

            add_ticket_textarea.addEventListener("keydown", async (e) => {
              if (e.key == 'Enter' && !e.shiftKey) {
                  const ticket_id = await post_ticket(name, add_ticket_textarea.value);
                  console.log("adding ticket with id: " + ticket_id);

                  const col_id = add_ticket_textarea.id.split("_")[1];
                  console.log(col_id + " textarea submitted");
                  add_ticket(col_id, ticket_id, add_ticket_textarea.value);
                  add_ticket_textarea.value = "";
              }
              update_height();
            });
            add_ticket_textarea.addEventListener("input", update_height);
        }

        col.id = "col_" + name;
        col.classList.add('column');
        containerDiv.appendChild(col);
        console.log("adding col: " + name);
    }
}

var mouseover_ticket;

function add_ticket(col_id, ticket_id, text, votes) {
    const column = document.getElementById("tickets_" + col_id);
    const prev_ticket = document.getElementById("ticket_" + ticket_id);
    if (null == prev_ticket) {
        const ticket = document.createElement('div');
        const content = document.createElement('p');
        ticket.appendChild(content);
        content.textContent = text;

        {
            //Initiate vote counter
            const vote_div = document.createElement('div');
            vote_div.classList.add('vote_div');
            vote_div.id = 'vote_div_' + ticket_id;
            const vote_count = document.createElement('span');
            vote_count.classList.add('vote_count');
            vote_count.id = "vote_count_" + ticket_id;
            vote_div.appendChild(vote_count);
            if (votes && votes > 0) {
                vote_count.textContent = votes;
            } else {
                vote_div.classList.add('hidden_button');
            }

            const dec_button = document.createElement('button');
            {
                vote_div.appendChild(dec_button);
                dec_button.textContent = '-';
                dec_button.classList.add('hidden_button');
                dec_button.addEventListener('click', function (e) {
                    post_dec_vote(ticket_id);
                });
            }

            const inc_button = document.createElement('button');
            {
                vote_div.appendChild(inc_button);
                inc_button.textContent = '+';
                inc_button.classList.add('hidden_button');
                inc_button.addEventListener('click', function (e) {
                    post_inc_vote(ticket_id);
                });
                ticket.appendChild(vote_div);
            }

            ticket.addEventListener("mouseenter", function(e) {
                mouseover_ticket = ticket_id;
                inc_button.classList.remove('hidden_button');
                dec_button.classList.remove('hidden_button');
                vote_div.classList.remove('hidden_button');
            });

            ticket.addEventListener("mouseleave", function(e) {
                mouseover_ticket = "";
                inc_button.classList.add('hidden_button');
                dec_button.classList.add('hidden_button');
                const current = parseInt(vote_count.textContent) || 0;
                if (current == 0) {
                    vote_div.classList.add('hidden_button');
                }
            });
        }

        ticket.classList.add("ticket");
        ticket.id = "ticket_" + ticket_id;
        column.appendChild(ticket);

        ticket.addEventListener("click", function(e) {
            console.log("ticket onclick: " + e.getModifierState("Control"));
            if (e.getModifierState("Control")) {
                ticket.classList.toggle(sel_class);
                //console.log("ticket onclick2: " + e);
                if (ticket.classList.contains(sel_class)) {
                //    ticket.classList.remove(sel_class);
                    add_selected(ticket.id);
                } else {
                    //ticket.classList.add(sel_class);
                    rm_selected(ticket.id);
                }
                //console.log("classes: " + ticket.classList);
            }
        });

        ticket.add

    } else {
      prev_ticket.firstElementChild.textContent = text;
      console.log("ticket already exits: " + ticket_id + ", text: " + text);
    }
}

function board_url(suffix) {
    return `/board/${board_id}/${suffix}`;
}

function ticket_url(ticket_id, suffix) {
    const ticket = `ticket/${ticket_id}/${suffix}`;
    return board_url(ticket);
}

async function post_update_ticket(ticket_id, col_id, text) {
    let body = JSON.stringify({
            col_id: col_id,
            text: text,
        });
    const response = await fetch(ticket_url(ticket_id, 'update'), {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json;charset=utf-8'
        },
        body: body,
    });
    await response.ok;
    add_ticket(col_id, ticket_id, text);
};

async function fetch_ticket(ticket_id) {
    const response = await fetch(ticket_url(ticket_id, 'fetch'));
    const json = await response.json();
    const ticket = json[0];
    const votes = json[1];

    //console.log("ticket.text: " + ticket.text);
    //console.log("ticket.col_id: " + ticket.col_id);
    add_ticket(ticket.col_id, ticket_id, ticket.text, votes);
};

async function post_ticket(col_id, text) {
    let response = await fetch(board_url('add_ticket'), {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json;charset=utf-8'
        },
        body: JSON.stringify({
            col_id: col_id,
            text: text,
        }),
    });
    let ticket_id = await response.json();
    return ticket_id;
}

async function post_rm_ticket(ticket_id) {
    const response = await fetch(ticket_url(ticket_id, 'rm'), {
        method: 'POST',
    });
    return response.ok;
}

async function fetch_board() {
    const response = await fetch(board_url('fetch'));
    const board = await response.json();
    console.log("fetch_board: " + board);
    return board;
};

async function post_inc_vote(ticket_id) {
    const response = await fetch(ticket_url(ticket_id, 'inc_vote'), {
        method: 'POST',
    });
    //const board = await response.json();
    console.log("post_inc_vote: " + response.ok);
    //return board;
};

async function post_dec_vote(ticket_id) {
    const response = await fetch(ticket_url(ticket_id, 'dec_vote'), {
        method: 'POST',
    });
    //const board = await response.json();
    console.log("post_dec_vote: " + response.ok);
    //return board;
};

function login(user) {
    username_display.textContent = user;
    initialise();

    const websocket = new WebSocket(`ws://${window.location.host}/board/${board_id}/websocket`);
    websocket.onopen = function() {
        console.log("connection opened");
        websocket.send(user);
    }

    websocket.onclose = function() {
        console.log("connection closed");
    }

    websocket.onmessage = on_websocket_message;
}

function on_websocket_message(e) {
    console.log("received message: "+e.data);
    const parts = e.data.split(": ");
    const msg_type = parts[0];
    const remaining = parts[1];
    switch (msg_type) {
        case "inc_vote": {
            console.log("inc_vote: " + remaining);
            const vote_count = document.getElementById("vote_count_" + remaining);
            if (!vote_count) break;
            var current = parseInt(vote_count.textContent) || 0;
            console.log("current: " + current);
            vote_count.textContent = current + 1;
            const vote_div = document.getElementById("vote_div_" + remaining);
            if (vote_div) vote_div.classList.remove('hidden_button');
            break;
        }
        case "dec_vote": {
            const vote_count = document.getElementById("vote_count_" + remaining);
            if (!vote_count) break;
            var current = parseInt(vote_count.textContent) || 0;
            if (!current || current == 0) {
                break;
            }
            console.log("current: " + current);

            const new_count = current - 1;
            vote_count.textContent = new_count;
            //Hide button if new_count is 0 and we aren't mousing over it now
            if (new_count == 0 && mouseover_ticket != remaining) {
                const vote_div = document.getElementById("vote_div_" + remaining);
                if (vote_div) {
                    vote_div.classList.add('hidden_button');
                }
            }

            break;
        }
        case "add_ticket":
        case "update_ticket":
            fetch_ticket(remaining);
            break;
        case "rm_ticket": {
            const element = document.getElementById("ticket_" + remaining);
                console.log("rm: " + remaining);
            if (null != element) {
                element.remove();
            }
            break;
        }
        case "connected":
            console.log(peers.textContent);

            if (remaining == username_display.textContent) {
                return;
            }
            client_set.add(remaining);
            update_peers();
            break;

        case "disconnected":
            client_set.delete(remaining);
            update_peers();
            break;

        default:
            console.log("unknown message type: " + msg_type);
    }
}

console.log("cookie: " + document.cookie);
if (document.cookie != null) {
    const start = document.cookie.indexOf('SESSION=');
    if (start != -1) {
        console.log("cookie start: " + start);
        var end = document.cookie.substr(start).indexOf(';');
        console.log("cookie end1: " + end);
        if (end == -1) {
            end = document.cookie.substr(start).indexOf(' ');
            console.log("cookie end2: " + end);
        }

        end = end == -1 ? document.cookie.length : end;
        const cookie = document.cookie.substr(start, end);
        console.log("cookieuser: " + cookie);
        const user = document.cookie.split("=")[1];
        console.log("user: " + user);
        login(user);
    }
}

join_btn.addEventListener("click", async function(e) {
    login(username_input.value);
});

}//main
