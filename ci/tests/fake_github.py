"""A minimal stand-in for the GitHub releases API, for ci/tests only.

Serves on 127.0.0.1 and keeps one repository's releases in memory, with
just the endpoints ci/release-upload.sh uses. Prints its port on stdout,
then serves until killed. State can be seeded from a JSON file given as the
first argument: {"releases": [{"tag_name": ..., "assets": {name: bytes}}]}.
"""

import json
import re
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

RELEASES = []  # each: {"id", "tag_name", "draft", "assets": [{"id","name","size","state","data"}]}
NEXT = {"id": 1}


def new_id():
    NEXT["id"] += 1
    return NEXT["id"]


def public(release, base):
    return {
        "id": release["id"],
        "tag_name": release["tag_name"],
        "draft": release["draft"],
    }


def asset_json(asset, base):
    return {
        "id": asset["id"],
        "name": asset["name"],
        "size": asset["size"],
        "state": asset["state"],
        "browser_download_url": f"{base}/download/{asset['id']}",
    }


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def base(self):
        return f"http://127.0.0.1:{self.server.server_address[1]}"

    def reply(self, code, body=b"", ctype="application/json"):
        if not isinstance(body, bytes):
            body = json.dumps(body).encode()
        self.send_response(code)
        self.send_header("content-type", ctype)
        self.send_header("content-length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def authed(self):
        return self.headers.get("authorization", "").startswith("token ")

    def body(self):
        n = int(self.headers.get("content-length", "0"))
        return self.rfile.read(n)

    def do_GET(self):
        path = urlparse(self.path).path
        m = re.fullmatch(r"/download/(\d+)", path)
        if m:
            for r in RELEASES:
                for a in r["assets"]:
                    if a["id"] == int(m.group(1)):
                        return self.reply(200, a["data"], "application/octet-stream")
            return self.reply(404)
        if not self.authed():
            return self.reply(401)
        m = re.fullmatch(r"/repos/[^/]+/[^/]+/releases/tags/(.+)", path)
        if m:
            for r in RELEASES:
                if r["tag_name"] == m.group(1) and not r["draft"]:
                    return self.reply(200, public(r, self.base()))
            return self.reply(404, {"message": "Not Found"})
        m = re.fullmatch(r"/repos/[^/]+/[^/]+/releases/(\d+)/assets", path)
        if m:
            for r in RELEASES:
                if r["id"] == int(m.group(1)):
                    return self.reply(200, [asset_json(a, self.base()) for a in r["assets"]])
            return self.reply(404)
        self.reply(404)

    def do_POST(self):
        if not self.authed():
            return self.reply(401)
        url = urlparse(self.path)
        if re.fullmatch(r"/repos/[^/]+/[^/]+/releases", url.path):
            data = json.loads(self.body())
            if any(r["tag_name"] == data["tag_name"] for r in RELEASES):
                return self.reply(422, {"message": "already_exists"})
            r = {"id": new_id(), "tag_name": data["tag_name"], "draft": bool(data.get("draft")), "assets": []}
            RELEASES.append(r)
            return self.reply(201, public(r, self.base()))
        m = re.fullmatch(r"/repos/[^/]+/[^/]+/releases/(\d+)/assets", url.path)
        if m:
            name = parse_qs(url.query)["name"][0]
            data = self.body()
            for r in RELEASES:
                if r["id"] == int(m.group(1)):
                    if any(a["name"] == name for a in r["assets"]):
                        return self.reply(422, {"message": "already_exists"})
                    a = {"id": new_id(), "name": name, "size": len(data), "state": "uploaded", "data": data}
                    r["assets"].append(a)
                    return self.reply(201, asset_json(a, self.base()))
            return self.reply(404)
        self.reply(404)

    def do_DELETE(self):
        if not self.authed():
            return self.reply(401)
        m = re.fullmatch(r"/repos/[^/]+/[^/]+/releases/assets/(\d+)", urlparse(self.path).path)
        if m:
            for r in RELEASES:
                for a in list(r["assets"]):
                    if a["id"] == int(m.group(1)):
                        r["assets"].remove(a)
                        return self.reply(204)
        self.reply(404)


def main():
    if len(sys.argv) > 1:
        seed = json.load(open(sys.argv[1]))
        for rel in seed.get("releases", []):
            r = {"id": new_id(), "tag_name": rel["tag_name"], "draft": False, "assets": []}
            for name, text in rel.get("assets", {}).items():
                data = text.encode()
                r["assets"].append({"id": new_id(), "name": name, "size": len(data), "state": "uploaded", "data": data})
            RELEASES.append(r)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    print(server.server_address[1], flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
