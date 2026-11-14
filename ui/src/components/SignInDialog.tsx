import { useState } from "react";

import { Button, Field, Input, NestedDialog } from "./ui";

/**
 * Asks which account a new sign-in token is for before `claude setup-token` runs: Claude Code
 * can't tell whose account a token is, so sessions show this name instead.
 */
export function SignInDialog({ onSignIn, onClose }: { onSignIn: (name: string) => void; onClose: () => void }) {
  const [name, setName] = useState("");
  const submit = () => {
    onSignIn(name.trim());
    onClose();
  };

  return (
    <NestedDialog
      title="Sign in to Claude Code"
      onClose={onClose}
      width={420}
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={submit}>
            Continue in browser
          </Button>
        </>
      }
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <Field label="Account name" hint="Shown on sessions that use this sign-in. Then approve in your browser with that account.">
          <Input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="work@company.com"
          />
        </Field>
      </form>
    </NestedDialog>
  );
}
