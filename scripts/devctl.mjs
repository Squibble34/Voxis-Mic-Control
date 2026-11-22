import http from "node:http"
import readline from "node:readline"

const DEV_HOST = "127.0.0.1"
const DEV_PORT = 47329

function sendCommand(cmd) {
  return new Promise((resolve, reject) => {
    const body = Buffer.from(cmd, "utf8")
    const req = http.request(
      {
        hostname: DEV_HOST,
        port: DEV_PORT,
        path: "/",
        method: "POST",
        headers: {
          "Content-Type": "text/plain; charset=utf-8",
          "Content-Length": body.length,
        },
      },
      (res) => {
        let responseText = ""
        res.setEncoding("utf8")
        res.on("data", (chunk) => {
          responseText += chunk
        })
        res.on("end", () => resolve(responseText))
      },
    )
    req.on("error", (err) => reject(err))
    req.write(body)
    req.end()
  })
}

function printConnectError(err) {
  const detail = err?.code || err?.message || "unknown error"
  console.error(
    `Could not connect to Voxis dev socket on ${DEV_HOST}:${DEV_PORT} (${detail}).\n` +
      `Make sure 'npm run tauri dev' has finished launching and prints '[dev] control socket ready on ${DEV_HOST}:${DEV_PORT}'.`,
  )
}

const args = process.argv.slice(2)
if (args.length > 0) {
  try {
    const reply = await sendCommand(args.join(" "))
    if (reply) console.log(reply)
  } catch (err) {
    printConnectError(err)
    process.exit(1)
  }
} else {
  try {
    await sendCommand("ping")
  } catch (err) {
    printConnectError(err)
    process.exit(1)
  }

  console.log("Voxis dev console connected - type 'help' for commands, 'Ctrl+C' to exit.")
  const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
    prompt: "voxis-dev> ",
  })
  rl.prompt()
  rl.on("line", async (line) => {
    const trimmed = line.trim()
    if (trimmed) {
      try {
        const reply = await sendCommand(trimmed)
        if (reply) console.log(reply)
      } catch (err) {
        printConnectError(err)
      }
    }
    rl.prompt()
  })
}
