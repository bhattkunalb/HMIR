"""
NPU Model Downloader for HMIR.

This script manages the synchronization of OpenVINO-optimized model weights
from Hugging Face Hub to the local machine, specifically targeting Intel NPU
acceleration formats.
"""
import os
import sys
from huggingface_hub import snapshot_download  # pylint: disable=import-error

# ── Default Model ───────────────────────────────────────────────────
DEFAULT_REPO  = "OpenVINO/qwen2.5-1.5b-instruct-int4-ov"
DEFAULT_FOLDER = "qwen2.5-1.5b-instruct-int4-ov"

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

def download_model(repo_id, folder_name):
    """
    Download and synchronize a model from Hugging Face Hub.

    Args:
        repo_id (str): The Hugging Face repository identifier.
        folder_name (str): The local folder name to store the model.
    """
    target_dir = os.path.join(get_data_dir(), folder_name)
    os.makedirs(target_dir, exist_ok=True)

    print(f"Downloading NPU model {repo_id} → {target_dir} ...")

    try:
        snapshot_download(
            repo_id=repo_id,
            local_dir=target_dir,
        )
        print(f"✅ NPU Model {repo_id} fully synchronized to {target_dir}")
    except Exception as e:  # pylint: disable=broad-except
        print(f"❌ Download failed: {e}")
        sys.exit(1)

if __name__ == "__main__":
    if len(sys.argv) >= 3:
        download_model(sys.argv[1], sys.argv[2])
    elif len(sys.argv) == 2:
        # User passed just a HF repo id, derive folder name from it
        repo = sys.argv[1]
        folder = repo.split("/")[-1]
        download_model(repo, folder)
    else:
        download_model(DEFAULT_REPO, DEFAULT_FOLDER)
