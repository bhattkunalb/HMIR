from huggingface_hub import snapshot_download

snapshot_download(
    repo_id="OpenVINO/qwen2.5-1.5b-instruct-int4-ov",
    local_dir="model",
    local_dir_use_symlinks=False
)