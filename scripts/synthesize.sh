#!/bin/bash

if [ -z "$1" ]; then
  echo "Usage: $0 \"Your text here\""
  exit 1
fi

curl -X POST "http://127.0.0.1:3000/synthesize" \
     -H "Content-Type: application/json" \
     -d "{\"text\": \"$1\"}" \
    --output "output.wav"
     
