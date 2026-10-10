import http.server, socketserver, json, os
ZIP="/tmp/pack.zip"; IDX="/tmp/idx.json"
class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if "community-index" in self.path:
            data=open(IDX,'rb').read(); ctype="application/json"
        else:
            data=open(ZIP,'rb').read(); ctype="application/zip"
        self.send_response(200)
        self.send_header("Content-Type",ctype)
        self.send_header("Content-Length",str(len(data)))
        self.end_headers()
        self.wfile.write(data)
    def log_message(self, fmt, *a):
        print("[packserver]", self.path[:90], flush=True)
socketserver.TCPServer.allow_reuse_address=True
with socketserver.ThreadingTCPServer(("0.0.0.0",8099), H) as httpd:
    print("serving on 0.0.0.0:8099", flush=True)
    httpd.serve_forever()
