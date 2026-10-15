#!/bin/sh
# Throwaway: cargo check each toolkit-free crate for each foreign target.
export RUSTUP_HOME=/tmp/dec11-rustup CARGO_HOME=/tmp/dec11-cargo PATH=/tmp/dec11-cargo/bin:$PATH
export CARGO_TARGET_DIR=/tmp/dec11-target
cd "$(dirname "$0")/../../.."
D=spikes/dec-11/inventory/logs
for t in x86_64-pc-windows-gnu x86_64-pc-windows-msvc aarch64-apple-darwin; do
 for c in suite-core suite-file suite-platform suite-ui csv-engine data-model formula xlsx grid commands file-format jobs document-model docx document-edit flow presentation-model pptx deck-edit image-model image-io; do
  s=$(date +%s); if cargo check --all-targets --target $t -p $c >$D/$t-$c.log 2>&1; then r=ok; else r=FAIL; fi
  echo "$t $c $r $(( $(date +%s)-s ))s"
 done
done
