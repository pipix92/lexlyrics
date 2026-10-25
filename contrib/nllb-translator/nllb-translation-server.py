#!/usr/bin/env python3
"""
NLLB-200 Translation Server with Dynamic VRAM Tiering
- Kept in system RAM by default (0 MB VRAM used).
- Auto-promotes to GPU VRAM (RTX 3050 CUDA) in ~0.7s when Waylyrics is running or on dashboard demand.
- Auto-offloads back to CPU RAM when Waylyrics closes and idle for > 60s, releasing VRAM to 0 MB.
Listens on http://127.0.0.1:18436
"""

import os
import sys
import time
import json
import logging
import threading
import subprocess

# Force 100% offline mode - do not contact HuggingFace Hub
# (override with TRANSFORMERS_OFFLINE=0 HF_HUB_OFFLINE=0 for the first run,
# to download the model once)
os.environ.setdefault("TRANSFORMERS_OFFLINE", "1")
os.environ.setdefault("HF_HUB_OFFLINE", "1")

from http.server import HTTPServer, BaseHTTPRequestHandler
import torch
from transformers import AutoModelForSeq2SeqLM, AutoTokenizer

PORT = 18436
MODEL_NAME = "facebook/nllb-200-distilled-600M"
CUDA_AVAILABLE = torch.cuda.is_available()

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
    datefmt="%H:%M:%S"
)
logger = logging.getLogger("nllb-server")

tokenizer = None
model = None
current_device = "cpu"
last_activity_time = 0.0
model_lock = threading.Lock()

stats = {
    "status": "initializing",
    "model": MODEL_NAME,
    "tier": "cpu",
    "device": "cpu",
    "translations_count": 0,
    "total_lines_translated": 0,
    "last_translation": None,
    "start_time": time.time()
}


def is_waylyrics_running() -> bool:
    try:
        # exact process names: a loose "lyrics" pattern also matched helper
        # scripts and kept the model in VRAM forever
        res = subprocess.run(["pgrep", "-x", "lexlyrics-bin|lexlyrics|waylyrics"],
                             capture_output=True, text=True)
        return res.returncode == 0
    except Exception:
        return False


def ensure_device(target_device: str):
    global model, current_device, stats
    if not CUDA_AVAILABLE and target_device == "cuda":
        return

    with model_lock:
        if current_device == target_device:
            return

        t0 = time.time()
        if target_device == "cuda":
            logger.info("Promoting NLLB-200: CPU RAM -> GPU VRAM (RTX 3050)...")
            model.to("cuda")
            current_device = "cuda"
            stats["tier"] = "vram"
            stats["device"] = "cuda"
            t1 = time.time()
            vram = torch.cuda.memory_allocated() / (1024 * 1024)
            logger.info(f"Promoted to VRAM in {t1 - t0:.3f}s! ({vram:.1f} MiB used)")
        else:
            logger.info("Offloading NLLB-200: GPU VRAM -> CPU RAM...")
            model.to("cpu")
            torch.cuda.empty_cache()
            current_device = "cpu"
            stats["tier"] = "ram"
            stats["device"] = "cpu"
            t1 = time.time()
            logger.info(f"Offloaded to CPU RAM in {t1 - t0:.3f}s! (0 MB VRAM used)")


def vram_manager_loop():
    """Background thread that dynamically manages VRAM based on Waylyrics state & activity."""
    global last_activity_time
    logger.info("VRAM Tiering Manager started.")
    while True:
        try:
            time.sleep(3.0)
            if not model:
                continue

            waylyrics_active = is_waylyrics_running()
            recent_activity = (time.time() - last_activity_time) < 75.0

            if waylyrics_active or recent_activity:
                if current_device != "cuda" and CUDA_AVAILABLE:
                    ensure_device("cuda")
            else:
                if current_device == "cuda":
                    logger.info("Waylyrics closed and no recent translation activity. Freeing VRAM...")
                    ensure_device("cpu")
        except Exception as e:
            logger.error(f"Error in VRAM manager loop: {e}")


def load_model():
    global tokenizer, model, current_device, stats
    logger.info(f"Pre-loading {MODEL_NAME} into CPU RAM...")
    t0 = time.time()
    tokenizer = AutoTokenizer.from_pretrained(MODEL_NAME)
    model = AutoModelForSeq2SeqLM.from_pretrained(MODEL_NAME, torch_dtype=torch.float16).to("cpu")
    model.eval()
    current_device = "cpu"
    t1 = time.time()
    stats["status"] = "online"
    stats["tier"] = "ram"
    stats["device"] = "cpu"
    stats["load_time_s"] = round(t1 - t0, 2)
    logger.info(f"NLLB-200 loaded in RAM in {t1 - t0:.2f}s! (VRAM: 0 MB)")

    # If Waylyrics is already running at server startup, promote to GPU immediately
    if is_waylyrics_running() and CUDA_AVAILABLE:
        ensure_device("cuda")


def translate_lines(lines: list[str], tgt_lang: str = "ita_Latn", src_lang: str = None) -> list[str]:
    global tokenizer, model, stats, last_activity_time
    if not lines:
        return []

    last_activity_time = time.time()

    # Ensure model is on CUDA for fast inference
    if CUDA_AVAILABLE and current_device != "cuda":
        ensure_device("cuda")

    dev = current_device
    tgt_id = tokenizer.convert_tokens_to_ids(tgt_lang)

    indexed_lines = [(i, line.strip()) for i, line in enumerate(lines) if line.strip()]
    if not indexed_lines:
        return [""] * len(lines)

    batch_texts = [item[1] for item in indexed_lines]

    with model_lock:
        if src_lang and hasattr(tokenizer, "src_lang"):
            tokenizer.src_lang = src_lang
        inputs = tokenizer(batch_texts, return_tensors="pt", padding=True, truncation=True, max_length=512).to(dev)
        with torch.no_grad():
            translated_tokens = model.generate(
                **inputs,
                forced_bos_token_id=tgt_id,
                max_length=512,
                num_beams=1
            )
        translated_texts = [tokenizer.decode(t, skip_special_tokens=True) for t in translated_tokens]

    results = [""] * len(lines)
    for (orig_idx, _), trans in zip(indexed_lines, translated_texts):
        results[orig_idx] = trans

    stats["translations_count"] += 1
    stats["total_lines_translated"] += len(lines)
    if results:
        stats["last_translation"] = {
            "orig": lines[0][:60],
            "trans": results[0][:60],
            "timestamp": time.time()
        }

    return results


class TranslationHandler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        if "GET /health" in args[0] or "GET /status" in args[0]:
            return
        super().log_message(format, *args)

    def do_OPTIONS(self):
        self.send_response(200)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "POST, GET, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type, X-Requested-With")
        self.end_headers()

    def do_GET(self):
        if self.path in ("/health", "/status"):
            vram_mb = 0.0
            if torch.cuda.is_available():
                vram_mb = round(torch.cuda.memory_allocated() / (1024 * 1024), 1)

            resp_data = {
                "status": stats["status"],
                "model": stats["model"],
                "tier": stats["tier"],
                "device": current_device,
                "vram_mb": vram_mb,
                "waylyrics_running": is_waylyrics_running(),
                "translations_count": stats["translations_count"],
                "total_lines_translated": stats["total_lines_translated"],
                "last_translation": stats["last_translation"],
                "uptime_s": round(time.time() - stats["start_time"], 1)
            }
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            self.wfile.write(json.dumps(resp_data).encode("utf-8"))
        elif self.path == "/wake":
            # Explicit wake up / pre-warm to GPU
            ensure_device("cuda")
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            self.wfile.write(b'{"status":"promoted_to_vram"}')
        else:
            self.send_response(404)
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()

    def do_POST(self):
        if self.path == "/translate":
            try:
                content_length = int(self.headers.get("Content-Length", 0))
                body = self.rfile.read(content_length)
                req = json.loads(body.decode("utf-8"))

                lines = req.get("texts", [])
                if isinstance(lines, str):
                    lines = [lines]

                tgt_lang = req.get("tgt_lang", "ita_Latn")
                src_lang = req.get("src_lang", None)

                t0 = time.time()
                translated = translate_lines(lines, tgt_lang=tgt_lang, src_lang=src_lang)
                t1 = time.time()

                resp_data = {
                    "translations": translated,
                    "count": len(translated),
                    "device": current_device,
                    "latency_ms": round((t1 - t0) * 1000, 1),
                    "model": stats["model"]
                }
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Access-Control-Allow-Origin", "*")
                self.end_headers()
                self.wfile.write(json.dumps(resp_data).encode("utf-8"))
            except Exception as e:
                logger.error(f"Error handling translation: {e}")
                self.send_response(500)
                self.send_header("Content-Type", "application/json")
                self.send_header("Access-Control-Allow-Origin", "*")
                self.end_headers()
                self.wfile.write(json.dumps({"error": str(e)}).encode("utf-8"))
        else:
            self.send_response(404)
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()


def run_server():
    load_model()
    # Start VRAM tiering thread
    t = threading.Thread(target=vram_manager_loop, daemon=True)
    t.start()

    server = HTTPServer(("127.0.0.1", PORT), TranslationHandler)
    logger.info(f"NLLB-200 Server ready at http://127.0.0.1:{PORT}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        logger.info("Server shutting down.")
        server.server_close()


if __name__ == "__main__":
    run_server()
