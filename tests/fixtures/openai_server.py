import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def do_GET(self):
        if self.path != "/v1/models":
            self.send_error(404)
            return
        self.reply({"object": "list", "data": [{"id": "local-test", "object": "model"}]})

    def do_POST(self):
        length = int(self.headers.get("content-length", "0"))
        request = json.loads(self.rfile.read(length) or b"{}")
        if self.path == "/v1/chat/completions":
            self.send_response(200)
            self.send_header("content-type", "text/event-stream")
            self.end_headers()
            event = {
                "id": "chat-local",
                "model": request.get("model", "local-test"),
                "choices": [
                    {"index": 0, "delta": {"content": "local answer"}, "finish_reason": "stop"}
                ],
            }
            self.wfile.write(("data: " + json.dumps(event) + "\n\ndata: [DONE]\n\n").encode())
            self.wfile.flush()
        elif self.path == "/v1/fim/completions":
            self.reply(
                {
                    "id": "fim-local",
                    "model": request.get("model", "local-test"),
                    "choices": [
                        {
                            "index": 0,
                            "message": {"role": "assistant", "content": "middle"},
                            "finish_reason": "stop",
                        }
                    ],
                }
            )
        else:
            self.send_error(404)

    def reply(self, value):
        body = json.dumps(value).encode()
        self.send_response(200)
        self.send_header("content-type", "application/json")
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)


ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), Handler).serve_forever()
