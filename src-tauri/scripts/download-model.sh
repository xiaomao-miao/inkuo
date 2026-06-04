#!/bin/bash
# Download BGE Embedding Models for inkuo Knowledge Base
#
# Usage:
#   ./download-model.sh [model_name]
#
# Supported models:
#   - BAAI/bge-small-zh-v1.5 (default, ~25MB ONNX)
#   - BAAI/bge-base-zh-v1.5 (~390MB ONNX)
#   - BAAI/bge-large-zh-v1.5 (~1.3GB ONNX)
#
# Downloads tokenizer + ONNX model files needed by fastembed.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODEL_DIR="$(dirname "$SCRIPT_DIR")/models"

MODEL_NAME="${1:-BAAI/bge-small-zh-v1.5}"

declare -A MODEL_DIMS
declare -A MODEL_SIZES
MODEL_DIMS["BAAI/bge-small-zh-v1.5"]="512"
MODEL_DIMS["BAAI/bge-base-zh-v1.5"]="768"
MODEL_DIMS["BAAI/bge-large-zh-v1.5"]="1024"
MODEL_SIZES["BAAI/bge-small-zh-v1.5"]="~25MB"
MODEL_SIZES["BAAI/bge-base-zh-v1.5"]="~390MB"
MODEL_SIZES["BAAI/bge-large-zh-v1.5"]="~1.3GB"

declare -A ONNX_REPOS
ONNX_REPOS["BAAI/bge-small-zh-v1.5"]="Xenova/bge-small-zh-v1.5"
ONNX_REPOS["BAAI/bge-base-zh-v1.5"]="Xenova/bge-base-zh-v1.5"
ONNX_REPOS["BAAI/bge-large-zh-v1.5"]="Xenova/bge-large-zh-v1.5"

if [[ -z "${MODEL_DIMS[$MODEL_NAME]}" ]]; then
    echo "Error: Unsupported model '$MODEL_NAME'"
    echo ""
    echo "Supported models:"
    echo "  - BAAI/bge-small-zh-v1.5 (default, ~25MB)"
    echo "  - BAAI/bge-base-zh-v1.5 (~390MB)"
    echo "  - BAAI/bge-large-zh-v1.5 (~1.3GB)"
    echo ""
    echo "Usage: ./download-model.sh [model_name]"
    exit 1
fi

MODEL_DIR_NAME="${MODEL_NAME//\//-}"
TARGET_DIR="$MODEL_DIR/$MODEL_DIR_NAME"
ONNX_REPO="${ONNX_REPOS[$MODEL_NAME]}"

echo "=========================================="
echo "Downloading Embedding Model"
echo "=========================================="
echo "Model: $MODEL_NAME"
echo "ONNX Repo: $ONNX_REPO"
echo "Dimensions: ${MODEL_DIMS[$MODEL_NAME]}"
echo "Size: ${MODEL_SIZES[$MODEL_NAME]}"
echo "Save to: $TARGET_DIR"
echo ""

mkdir -p "$TARGET_DIR"
cd "$TARGET_DIR"

HF_BASE="https://hf-mirror.com"

download_file() {
    local url=$1
    local filename=$2
    local max_retries=3
    local retry=0

    echo -n "Downloading $filename... "

    while [ $retry -lt $max_retries ]; do
        if curl -L --connect-timeout 30 --max-time 600 -o "$filename" "$url" 2>/dev/null; then
            if [ -s "$filename" ]; then
                if head -c 100 "$filename" | grep -qi "error\|404\|not found\|login"; then
                    echo "FAILED (got error page)"
                    rm -f "$filename"
                    retry=$((retry + 1))
                else
                    local size=$(du -h "$filename" | cut -f1)
                    echo "OK ($size)"
                    return 0
                fi
            fi
        else
            retry=$((retry + 1))
        fi
        echo "Retry $retry/$max_retries..."
        rm -f "$filename"
        sleep 3
    done

    echo "FAILED after $max_retries retries"
    return 1
}

echo "--- Downloading tokenizer & config from HuggingFace ---"

HF_ORIG="$HF_BASE/$MODEL_NAME/resolve/main"
download_file "$HF_ORIG/tokenizer.json" "tokenizer.json"
download_file "$HF_ORIG/tokenizer_config.json" "tokenizer_config.json"
download_file "$HF_ORIG/special_tokens_map.json" "special_tokens_map.json"
download_file "$HF_ORIG/config.json" "config.json"

if ! download_file "$HF_ORIG/vocab.txt" "vocab.txt" 2>/dev/null; then
    echo "vocab.txt not found for this model, skipping."
    rm -f vocab.txt
fi

echo ""
echo "--- Downloading ONNX model files ---"

HF_ONNX="$HF_BASE/$ONNX_REPO/resolve/main"
if download_file "$HF_ONNX/onnx/model.onnx" "model.onnx"; then
    echo "  ONNX model downloaded successfully"
else
    echo ""
    echo "ERROR: Failed to download ONNX model."
    echo "The model will not work without model.onnx"
fi

cat > model.json << EOF
{
    "model_name": "$MODEL_NAME",
    "dimensions": ${MODEL_DIMS[$MODEL_NAME]},
    "max_length": 512,
    "pooling": "mean",
    "normalize": true
}
EOF
echo "model.json created"

echo ""
echo "=========================================="
echo "Download Complete!"
echo "=========================================="
echo ""
echo "Files in $TARGET_DIR:"
ls -lh 2>/dev/null || echo "No files found"
echo ""
echo "Total size: $(du -sh . 2>/dev/null | cut -f1)"
echo ""
if [ -f "model.onnx" ]; then
    echo "ONNX model: present"
else
    echo "ONNX model: MISSING (download failed)"
fi
if [ -f "tokenizer.json" ]; then
    echo "Tokenizer: present"
else
    echo "Tokenizer: MISSING"
fi
echo ""
echo "Next: Restart the app and it should work."
