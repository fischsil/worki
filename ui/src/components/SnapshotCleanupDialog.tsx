import { useQueryClient } from "@tanstack/react-query";
import { LoaderCircle, Sparkles } from "lucide-react";
import { useState } from "react";

import { api, errorMessage } from "../lib/api";
import { keys } from "../lib/queries";
import { Button, ErrorText, NestedDialog, Textarea } from "./ui";

const SUGGESTION = `command -v sudo >/dev/null || sudo() { "$@"; }

# Only deletes what lives on the environment's own disk. Shared folders are mounts with another
# device: the worktree, .git, the Claude Code folders and $WORKI_PROJECT_DIR.
root=$(stat -c %d /)
clean() {
  for p in "$@"; do
    [ -e "$p" ] || continue
    real=$(realpath "$p")
    if [ "$(stat -c %d "$real")" != "$root" ]; then
      echo "Skipped $p: it's in a shared folder"
      continue
    fi
    sudo find "$real" -xdev -depth -delete 2>/dev/null
  done
  true
}

sudo apt-get clean 2>/dev/null
clean /var/lib/apt/lists/*

clean ~/.npm/_cacache ~/.cache/pip ~/.cache/uv ~/.cache/go-build ~/.cache/yarn ~/.node-gyp ~/.local/share/pnpm/store
clean /root/.cache /root/.npm

clean ~/.gradle/daemon ~/.gradle/caches/build-cache-* ~/.gradle/caches/*/fileHashes ~/.gradle/caches/journal-1
[ -d ~/.gradle/caches ] && clean $(find "$(realpath ~/.gradle/caches)" -xdev -name '*.lock' 2>/dev/null)

clean ~/.claude ~/.claude.json ~/.claude.json.backup ~/.config/claude
clean ~/.git-credentials ~/.netrc ~/.docker/config.json

clean /tmp/* /tmp/.[!.]* /var/tmp/*
sudo find /var/log -xdev -type f \\( -name '*.gz' -o -name '*.[0-9]' -o -name '*.old' \\) -delete
sudo find /var/log -xdev -type f -exec truncate -s 0 {} +
clean ~/.bash_history ~/.zsh_history ~/.python_history ~/.lesshst ~/.viminfo /root/.bash_history
`;

export function SnapshotCleanupDialog({ script, onClose }: { script: string; onClose: () => void }) {
  const qc = useQueryClient();
  const [text, setText] = useState(script);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function save() {
    setBusy(true);
    try {
      await api.setSnapshotCleanup(text);
      await qc.invalidateQueries({ queryKey: keys.snapshotCleanup });
      onClose();
    } catch (e) {
      setError(errorMessage(e));
      setBusy(false);
    }
  }

  return (
    <NestedDialog
      title="Cleanup before snapshots"
      onClose={onClose}
      width={680}
      footer={
        <>
          <Button
            variant="ghost"
            className="mr-auto"
            onClick={() => setText(SUGGESTION)}
            title="Puts a suggested cleanup in the editor. Nothing changes until you save."
          >
            <Sparkles size={12} />
            Use suggestion
          </Button>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={save} disabled={busy || text === script}>
            {busy && <LoaderCircle size={12} className="animate-spin" />}
            Save
          </Button>
        </>
      }
    >
      <p className="text-xs leading-relaxed text-muted">
        Runs in the home folder of the sandbox or container before each snapshot is saved, for every project, to keep
        snapshots small and free of sign-ins. If it fails, nothing is saved. The worktree and shared folders are
        mounted inside, so only delete the environment's own files; the suggestion skips anything shared. Deleting
        files only shrinks a snapshot when they were added after the image it started from.
      </p>
      <Textarea
        value={text}
        onChange={(e) => setText(e.target.value)}
        rows={18}
        placeholder="sudo apt-get clean"
        spellCheck={false}
        className="min-h-64 resize-y font-mono text-xs leading-relaxed"
        aria-label="Cleanup script"
      />
      <ErrorText>{error}</ErrorText>
    </NestedDialog>
  );
}
