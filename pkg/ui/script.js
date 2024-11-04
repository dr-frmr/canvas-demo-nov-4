const APP_PATH = '/canvas-demo:canvas-demo:template.os/api';

const canvas_container = document.getElementById('canvas');
canvas_container.innerHTML = '';
const canvas_element = document.createElement('canvas');
canvas_element.width = 500;
canvas_element.height = 500;
canvas_container.appendChild(canvas_element);
const ctx = canvas_element.getContext('2d');

const dropdown = document.getElementById('canvas-select');

function api_call(body) {
    return fetch(APP_PATH, {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json',
        },
        body: JSON.stringify(body),
    }).then(response => {
        if (response.headers.get('content-length') === '0') {
            return {};
        } else {
            return response.json();
        }
    });
}

// Setup WebSocket connection
const wsProtocol = location.protocol === 'https:' ? 'wss://' : 'ws://';
const ws = new WebSocket(wsProtocol + location.host + "/canvas-demo:canvas-demo:template.os/updates");
ws.onmessage = event => {
    const [canvas_id, point] = JSON.parse(event.data);
    if (dropdown.value === canvas_id) {
        // every update is an object with a color, x, and y
        ctx.fillStyle = point.color;
        ctx.fillRect(point.x, point.y, 1, 1);
    }
};

populate_dropdown();

document.getElementById('add-user-button').addEventListener('click', () => {
    const userInput = document.getElementById('add-user');
    const user = userInput.value.trim();
    if (user) {
        api_call({ "AddUser": user }).then(() => {
            userInput.value = '';
            // Refresh canvas to show updated users
            const canvas_id = document.getElementById('canvas-select').value;
            api_call({ "GetCanvas": canvas_id }).then(data => populate_canvas(canvas_id, data));
        });
    }
});

document.getElementById('remove-user-button').addEventListener('click', () => {
    const userInput = document.getElementById('remove-user');
    const user = userInput.value.trim();
    if (user) {
        api_call({ "RemoveUser": user }).then(() => {
            userInput.value = '';
            // Refresh canvas to show updated users
            const canvas_id = document.getElementById('canvas-select').value;
            api_call({ "GetCanvas": canvas_id }).then(data => populate_canvas(canvas_id, data));
        });
    }
});


function populate_dropdown() {
    api_call("GetCanvasList").then(data => {
        dropdown.innerHTML = data.map(entry => `<option value="${entry}">${entry}</option>`).join('');
        dropdown.value = window.our.node;
        let initial_canvas_id = dropdown.value;

        api_call({ "GetCanvas": initial_canvas_id }).then(data => populate_canvas(initial_canvas_id, data));

        dropdown.addEventListener('change', () => {
            const canvas_id = dropdown.value;
            api_call({ "GetCanvas": canvas_id }).then(data => populate_canvas(canvas_id, data));
        });
    });
}

function populate_canvas(canvas_id, canvas) {
    // if canvas is not our own, remove add/remove user buttons
    if (canvas_id !== window.our.node) {
        document.getElementById('canvas-controls').style.display = 'none';
    } else {
        document.getElementById('canvas-controls').style.display = 'block';
    }
    // set canvas to blank white
    ctx.fillStyle = 'white';
    ctx.fillRect(0, 0, canvas_element.width, canvas_element.height);

    // list the canvas users below the canvas
    const users_container = document.getElementById('users');
    users_container.innerHTML = "users: " + canvas.users.join(', ');

    // draw the canvas using the points in the canvas object
    canvas.points.forEach(point => {
        ctx.fillStyle = point.color;
        ctx.fillRect(point.x, point.y, 1, 1);
    });

    let isDrawing = false;
    let lastPoint = null;

    function draw(e) {
        if (!isDrawing) return;

        // Get position relative to canvas
        const rect = canvas_element.getBoundingClientRect();
        const x = e.clientX - rect.left;
        const y = e.clientY - rect.top;

        ctx.fillStyle = 'black';
        ctx.fillRect(x, y, 1, 1);

        let point = { x: Math.floor(x), y: Math.floor(y), color: 'black' };

        if (!lastPoint || (lastPoint.x !== point.x || lastPoint.y !== point.y)) {
            api_call({ "Draw": [canvas_id, point] });
            lastPoint = point;
        }
    }

    canvas_element.addEventListener('mousedown', (e) => {
        isDrawing = true;
    });

    canvas_element.addEventListener('mousemove', draw);
    canvas_element.addEventListener('mouseup', () => isDrawing = false);
    canvas_element.addEventListener('mouseout', () => isDrawing = false);
}