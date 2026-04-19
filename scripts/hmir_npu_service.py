# pylint: skip-file
"""
HMIR NPU Inference Service.
Native OpenVINO integration for Intel NPU-accelerated LLM execution.
"""

import os
import json
import asyncio
from aiohttp import web  # pylint: disable=import-error
from openvino_genai import LLMPipeline  # pylint: disable=import-error

# Configuration
PORT = 8089
MODEL_PATH = os.environ.get("HMIR_MODEL_PATH", "qwen2.5-1.5b-ov")

# Global Pipeline
pipeline = None

def init_pipeline():
    """Load the OpenVINO GenAI pipeline for the Intel NPU."""
    global pipeline
    try:
        if not os.path.exists(MODEL_PATH):
            print(f"ERROR: Model path not found: {MODEL_PATH}")
            return
        print(f"LOADING NPU MODEL: {MODEL_PATH}")
        pipeline = LLMPipeline(MODEL_PATH, "NPU")
        print("NPU MODEL LOADED SUCCESSFULLY")
    except RuntimeError as e:
        print(f"CRITICAL ERROR LOADING NPU: {e}")
    except (TypeError, ValueError) as e:
        print(f"CONFIGURATION ERROR: {e}")

async def handle_chat(request):
    """Handle POST /v1/chat/completions (streaming)."""
    try:
        body = await request.json()
        prompt = body["messages"][-1]["content"]

        response = web.StreamResponse(
            status=200,
            headers={"Content-Type": "text/event-stream"}
        )
        await response.prepare(request)

        loop = asyncio.get_running_loop()
        queue = asyncio.Queue()

        def streamer(sub_text):
            # Put token into the queue from the inference thread
            loop.call_soon_threadsafe(queue.put_nowait, sub_text)
            return False

        # Run blocking inference in a background thread
        async def run_inference():
            try:
                await asyncio.to_thread(pipeline.generate, prompt, max_new_tokens=512, streamer=streamer)
            finally:
                loop.call_soon_threadsafe(queue.put_nowait, None) # Signal end of stream

        inference_task = asyncio.create_task(run_inference())

        while True:
            token = await queue.get()
            if token is None:
                break
            
            chunk = {"choices": [{"delta": {"content": token}}]}
            await response.write(f"data: {json.dumps(chunk)}\n\n".encode("utf-8"))

        await response.write(b"data: [DONE]\n\n")
        await inference_task
        return response

    except json.JSONDecodeError:
        return web.json_response({"error": "Invalid JSON"}, status=400)
    except Exception as e:
        print(f"ERROR IN CHAT: {e}")
        return web.json_response({"error": str(e)}, status=500)

def check_health(_request):
    """Health check endpoint."""
    status = "READY" if pipeline else "INITIALIZING"
    return web.json_response({"status": status})

def main():
    """Start the NPU worker server."""
    app = web.Application()
    app.add_routes([
        web.post("/v1/chat/completions", handle_chat),
        web.get("/health", check_health)
    ])
    init_pipeline()
    web.run_app(app, port=PORT, access_log=None)

if __name__ == "__main__":
    main()
