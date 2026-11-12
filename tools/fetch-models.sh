#!/bin/sh
# Downloads the receipt reader models (PaddleOCR PP-OCRv6 Tiny, Apache-2.0, about 6 MB) into ./models.
set -eu
cd "$(dirname "$0")/.."
mkdir -p models
base=https://github.com/GreatV/oar-ocr/releases/download/v0.7.0
for f in pp-ocrv6_tiny_det.onnx pp-ocrv6_tiny_rec.onnx ppocrv6_tiny_dict.txt; do
  [ -s "models/$f" ] || curl -fsSL -o "models/$f" "$base/$f"
done
ls -la models
