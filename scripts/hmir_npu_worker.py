"""
HMIR NPU Inference Worker.

This module provides an HTTP server (via aiohttp) that proxies chat completion
requests to the Intel NPU using OpenVINO GenAI. It implements the ChatML
prompt template for Qwen2.5 models and supports streaming responses.
"""

import os
import sys
import json
import asyncio
from aiohttp import web  # pylint: disable=import-error
import openvino_genai as ovg  # pylint: disable=import-error

# ── Model Configuration ─────────────────────────────────────────────
# Default to Qwen2.5-1.5B (INT4 OpenVINO). Override via CLI arg or env.
DEFAULT_MODEL = "qwen2.5-1.5b-instruct-int4-ov"

def get_data_dir():
    """Get the OS-appropriate storage directory for HMIR models."""
    if sys.platform == "win32":
        base = os.environ.get("LOCALAPPDATA", os.path.expanduser("~\\AppData\\Local"))
    elif sys.platform == "darwin":
        base = os.path.expanduser("~/Library/Application Support")
    else:
        # Linux/Other: Follow XDG spec
        base = os.environ.get("XDG_DATA_HOME", os.path.expanduser("~/.local/share"))
    return os.path.join(base, "hmir", "models")

_MODEL_NAME = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("HMIR_NPU_MODEL", DEFAULT_MODEL)
_MODEL_DIR = os.path.join(get_data_dir(), _MODEL_NAME)

# Storage for background tasks to prevent garbage collection
_ACTIVE_TASKS = set()

# ── Prompt Formatting (ChatML for Qwen2.5) ──────────────────────────
def format_messages(messages):
    """
    Format a list of message objects into a ChatML prompt string.

    Args:
        messages: List of chat message dictionaries.

    Returns:
        A formatted ChatML string for Qwen2.5.
    """
    prompt = ""
    for m in messages:
        role = m.get("role", "user")
        content = m.get("content", "")
        prompt += f"<|im_start|>{role}\n{content}<|im_end|>\n"
    prompt += "<|im_start|>assistant\n"
    return prompt

# ── Pipeline Initialization ─────────────────────────────────────────
print(f"Loading OpenVINO Native Pipeline from {_MODEL_DIR}...")
print(f"  Model: {_MODEL_NAME}")
print("  Target Device: NPU (Accelerated)")
print("  Note: Model compilation may take 1-2 minutes on the first run.")
sys.stdout.flush()

try:
    print("  [1/2] Binding device... ", end="", flush=True)
    OV_PIPE = ovg.LLMPipeline(_MODEL_DIR, "NPU")
    print("OK")

    print("  [2/2] Configuring generation parameters... ", end="", flush=True)
    GENERATION_CONFIG = ovg.GenerationConfig()
    GENERATION_CONFIG.max_new_tokens = 512
    print("OK")

    print(f"\n✅ NPU Pipeline successfully bound and cached! ({_MODEL_NAME})")
    sys.stdout.flush()
except Exception as e:  # pylint: disable=broad-except
    print("Failed to load NPU pipeline. Ensure NPU drivers are intact and the model exists.")
    print(f"  Expected path: {_MODEL_DIR}")
    print(f"  Error: {e}")
    OV_PIPE = None

# ── Async Streamer (sync thread → async queue bridge) ───────────────
class AsyncStreamer(ovg.StreamerBase):
    """
    Bridge between synchronous OpenVINO generation and asynchronous aiohttp queue.
    """
    def __init__(self, queue: asyncio.Queue, loop: asyncio.AbstractEventLoop):
        """Initialize the streamer with an async queue and event loop."""
        super().__init__()
        self.queue = queue
        self.loop = loop

    def put(self, word: str) -> bool:
        """Called by OpenVINO for each new generated token."""
        # Pushing into async queue from sync thread
        self.loop.call_soon_threadsafe(self.queue.put_nowait, word)
        return False  # False means continue generation

    def end(self):
        """Signals the end of generation."""
        self.loop.call_soon_threadsafe(self.queue.put_nowait, None)

# ── HTTP Handlers ───────────────────────────────────────────────────
async def handle_chat(request):
    """Handles OpenAI-compatible chat completion requests via POST."""
    if OV_PIPE is None:
        return web.json_response({"error": "NPU Pipeline Offline"}, status=500)

    data = await request.json()
    messages = data.get("messages", [])
    prompt = format_messages(messages)

    response = web.StreamResponse(
        status=200,
        reason='OK',
        headers={
            'Content-Type': 'text/event-stream',
            'Cache-Control': 'no-cache',
            'Connection': 'keep-alive'
        }
    )
    await response.prepare(request)

    queue = asyncio.Queue()
    loop = asyncio.get_running_loop()
    streamer = AsyncStreamer(queue, loop)

    def run_generation():
        try:
            OV_PIPE.generate(prompt, GENERATION_CONFIG, streamer)
        finally:
            streamer.end()

    # Launch generation in a background thread so the event loop is unblocked
    task = asyncio.create_task(asyncio.to_thread(run_generation))
    _ACTIVE_TASKS.add(task)
    task.add_done_callback(_ACTIVE_TASKS.discard)

    # Read from queue and stream to client
    while True:
        token_str = await queue.get()
        if token_str is None:
            break

        chunk = {
            "choices": [{"delta": {"content": token_str}}]
        }
        await response.write(f"data: {json.dumps(chunk)}\n\n".encode('utf-8'))

    await response.write(b"data: [DONE]\n\n")
    return response

def handle_health(_request):
    """Returns the current status of the NPU worker."""
    return web.json_response({
        "status": "online" if OV_PIPE else "offline",
        "model": _MODEL_NAME,
        "device": "NPU"
    })

# ── Application Setup ───────────────────────────────────────────────
APP = web.Application()
APP.router.add_post('/v1/chat/completions', handle_chat)
APP.router.add_get('/health', handle_health)

if __name__ == '__main__':
    print("Starting HMIR NPU Worker on http://127.0.0.1:8089")
    web.run_app(APP, host='127.0.0.1', port=8089)
