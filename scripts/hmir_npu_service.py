# pylint: skip-file
"""
HMIR NPU Inference Service.
Native OpenVINO integration for Intel NPU-accelerated LLM execution.
Serves an OpenAI-compatible streaming endpoint on port 8089.
"""

import os
import sys
import json
import asyncio
import argparse
import platform
import shutil
from aiohttp import web  # pylint: disable=import-error
from openvino_genai import LLMPipeline  # pylint: disable=import-error

# Configuration
DEFAULT_PORT = 8089

def get_default_models_dir():
    """Get the standard HMIR models directory based on OS."""
    if platform.system() == "Windows":
        return os.path.join(os.environ.get("LOCALAPPDATA", ""), "hmir", "models")
    return os.path.expanduser("~/.local/share/hmir/models")

def resolve_model_path(path):
    """Resolve model path locally or in the standard models directory."""
    if os.path.exists(path):
        return os.path.abspath(path)

    # Try standard models directory
    models_dir = get_default_models_dir()
    alt_path = os.path.join(models_dir, path)
    if os.path.exists(alt_path):
        return os.path.abspath(alt_path)

    return path

# Global Pipeline
pipeline = None
pipeline_lock = asyncio.Lock()
current_model_path = None
service_status = "INITIALIZING"  # INITIALIZING | READY | LOADING | ERROR

def purge_model_cache(model_path):
    """Attempt to find and purge OpenVINO cache folders in the model directory."""
    cache_purged = False
    # OpenVINO GenAI often creates a folder named 'cl_cache' or similar in the model dir or CWD
    # We'll check the model dir for 'cache' or 'cl_cache' folders
    for cache_name in ["cache", "cl_cache", "blob_cache"]:
        potential_cache = os.path.join(model_path, cache_name)
        if os.path.exists(potential_cache) and os.path.isdir(potential_cache):
            print(f"[NPU-SERVICE] Purging corrupt cache: {potential_cache}", flush=True)
            try:
                shutil.rmtree(potential_cache)
                cache_purged = True
            except Exception as e:
                print(f"[NPU-SERVICE] Failed to purge {potential_cache}: {e}", flush=True)
    return cache_purged

def handle_pipeline_error(e, resolved_path, model_path, retry_on_cache_fail):
    """Handle errors occurring during pipeline initialization."""
    global service_status, pipeline
    err_msg = str(e)
    print(f"[NPU-SERVICE] CRITICAL - Failed to load on NPU: {err_msg}", flush=True)

    if "Cache entry deserialization failed" in err_msg:
        print("[NPU-SERVICE] HINT: Corrupt OpenVINO cache detected.", flush=True)
        if retry_on_cache_fail and purge_model_cache(resolved_path):
            print("[NPU-SERVICE] Cache purged. Retrying model load...", flush=True)
            return init_pipeline(model_path, retry_on_cache_fail=False)
    elif "Device with name 'NPU' is not registered" in err_msg:
        print("[NPU-SERVICE] HINT: NPU hardware not found or driver missing. Ensure Intel AI Boost is enabled in BIOS.", flush=True)

    if pipeline is None:
        service_status = "ERROR"
    return False

def init_pipeline(model_path=None, retry_on_cache_fail=True):
    """Load the OpenVINO GenAI pipeline for the Intel NPU."""
    global pipeline, current_model_path, service_status

    target_path = model_path or os.environ.get("HMIR_MODEL_PATH", "qwen2.5-1.5b-ov")
    resolved_path = resolve_model_path(target_path)

    if not os.path.exists(resolved_path):
        print(f"[NPU-SERVICE] ERROR: Model path not found: {resolved_path}", flush=True)
        service_status = "ERROR"
        return False

    print(f"[NPU-SERVICE] Loading model from: {resolved_path}", flush=True)
    print("[NPU-SERVICE] Target device: NPU", flush=True)
    service_status = "LOADING"

    try:
        new_pipeline = LLMPipeline(resolved_path, "NPU")
        pipeline = new_pipeline
        current_model_path = resolved_path
        service_status = "READY"
        print("[NPU-SERVICE] Model loaded successfully on NPU", flush=True)
        return True
    except Exception as e:
        return handle_pipeline_error(e, resolved_path, model_path, retry_on_cache_fail)

async def handle_load_model(request):
    """Handle POST /v1/engine/load."""
    global service_status
    try:
        body = await request.json()
        model_name = body.get("name")
        if not model_name:
            return web.json_response({"error": "Missing model name"}, status=400)

        async with pipeline_lock:
            service_status = "LOADING"
            success = await asyncio.to_thread(init_pipeline, model_name)

        if success:
            return web.json_response({"status": "success", "model": model_name})
        else:
            return web.json_response(
                {"error": f"Failed to load model {model_name}. Check NPU drivers and model path."},
                status=500
            )
    except Exception as e:
        return web.json_response({"error": str(e)}, status=500)

async def handle_chat(request):
    """Handle POST /v1/chat/completions (streaming)."""
    current_pipeline = pipeline

    if current_pipeline is None:
        return web.json_response({
            "error": "NPU Pipeline not initialized. Check hardware drivers or reload model."
        }, status=503)

    try:
        body = await request.json()
        messages = body.get("messages", [])
        if not messages:
            return web.json_response({"error": "Empty messages"}, status=400)

        prompt = messages[-1]["content"]
        max_tokens = body.get("max_tokens", 512)

        response = web.StreamResponse(
            status=200,
            headers={
                "Content-Type": "text/event-stream",
                "Cache-Control": "no-cache",
                "Access-Control-Allow-Origin": "*",
            }
        )
        await response.prepare(request)

        loop = asyncio.get_running_loop()
        queue = asyncio.Queue()

        def streamer(sub_text):
            if sub_text:
                # print(f".", end="", flush=True) # Too noisy for logs?
                loop.call_soon_threadsafe(queue.put_nowait, sub_text)
            return False

        async def run_inference(pipe_ref):
            global service_status
            service_status = "GENERATING"
            try:
                await asyncio.to_thread(
                    pipe_ref.generate, prompt,
                    max_new_tokens=min(max_tokens, 2048),
                    streamer=streamer
                )
            except Exception as e:
                print(f"[NPU-SERVICE] INFERENCE ERROR: {e}", flush=True)
                loop.call_soon_threadsafe(queue.put_nowait, RuntimeError(str(e)))
            finally:
                service_status = "READY"
                loop.call_soon_threadsafe(queue.put_nowait, None)

        inference_task = asyncio.create_task(run_inference(current_pipeline))

        while True:
            token = await queue.get()
            if token is None:
                break
            if isinstance(token, Exception):
                err_chunk = {"error": str(token)}
                await response.write(f"data: {json.dumps(err_chunk)}\n\n".encode("utf-8"))
                break

            chunk = {"choices": [{"delta": {"content": token}}]}
            await response.write(f"data: {json.dumps(chunk)}\n\n".encode("utf-8"))

        await response.write(b"data: [DONE]\n\n")
        await inference_task
        return response

    except json.JSONDecodeError:
        return web.json_response({"error": "Invalid JSON"}, status=400)
    except Exception as e:
        print(f"[NPU-SERVICE] ERROR IN CHAT: {e}", flush=True)
        try:
            return web.json_response({"error": str(e)}, status=500)
        except Exception:
            return web.Response(status=500)

def check_health(_request):
    """Health check endpoint."""
    return web.json_response({
        "status": service_status,
        "model": current_model_path,
        "device": "NPU",
    })

def main():
    """Start the NPU worker server."""
    parser = argparse.ArgumentParser(description="HMIR NPU Inference Worker")
    parser.add_argument("--port", type=int, default=DEFAULT_PORT, help="Port to listen on")
    parser.add_argument("--model", type=str, default=None, help="Model path or name to load")
    args = parser.parse_args()

    port = args.port

    print("=" * 60, flush=True)
    print("  HMIR NPU Inference Worker", flush=True)
    print(f"  Port: {port}", flush=True)
    print(f"  Python: {sys.executable}", flush=True)
    print(f"  CWD: {os.getcwd()}", flush=True)
    print(f"  Models dir: {get_default_models_dir()}", flush=True)
    print("=" * 60, flush=True)

    app = web.Application()
    app.add_routes([
        web.post("/v1/chat/completions", handle_chat),
        web.post("/v1/engine/load", handle_load_model),
        web.get("/health", check_health),
    ])

    # Initialize pipeline
    model_arg = args.model or os.environ.get("HMIR_MODEL_PATH", "qwen2.5-1.5b-ov")
    init_pipeline(model_arg)

    print(f"[NPU-SERVICE] Starting HTTP server on port {port}", flush=True)
    web.run_app(app, host="0.0.0.0", port=port, access_log=None)

if __name__ == "__main__":
    main()
