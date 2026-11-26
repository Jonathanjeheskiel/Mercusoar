// Simulated student PCs for testing and demos (synthetic data, not real students).
//
// Usage (from the app folder, while an assistant window hosts the hub):
//   node scripts/demo-students.mjs            -> 5 fake students send Flares
//   node scripts/demo-students.mjs 8          -> 8 fake students
//   node scripts/demo-students.mjs 5 192.168.1.10
// Press Ctrl+C to disconnect them. Their Flares stay in the queue until resolved.

const count = Number(process.argv[2] ?? 5);
const host = process.argv[3] ?? "127.0.0.1";
const PORT = 47800;

const QUESTIONS = [
  "Task 3: my CSS file is not loading",
  "Why does my flexbox not center the div?",
  "Error: Cannot read properties of undefined",
  "How do I link the JavaScript file to the HTML?",
  "My form submits but the page reloads",
  "",
  "Task 5: the image path is broken",
  "What is the difference between id and class?",
];

// Pick distinct student PCs (2-39) at random.
const pcs = [];
while (pcs.length < Math.min(count, 38)) {
  const pc = 2 + Math.floor(Math.random() * 38);
  if (!pcs.includes(pc)) pcs.push(pc);
}

pcs.forEach((pc, i) => {
  const ws = new WebSocket(`ws://${host}:${PORT}`);
  ws.onopen = () => {
    ws.send(JSON.stringify({ type: "hello_student", pc }));
    // Stagger the Flares so the queue order is visible.
    setTimeout(() => {
      ws.send(JSON.stringify({ type: "flare", question: QUESTIONS[i % QUESTIONS.length] }));
    }, 1500 * (i + 1));
  };
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data);
    if (msg.type === "my_flare" && msg.flare) {
      const who = msg.flare.claimed_by ? `helped by ${msg.flare.claimed_by}` : `#${msg.flare.position} in line`;
      console.log(`PC ${pc}: ${who}`);
    }
    if (msg.type === "resolved") console.log(`PC ${pc}: resolved`);
  };
  ws.onerror = () => console.log(`PC ${pc}: cannot reach the hub at ${host}:${PORT}`);
});

console.log(`Simulating ${pcs.length} students: PCs ${pcs.join(", ")}. Ctrl+C to stop.`);
