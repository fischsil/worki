import { useQuery, useQueryClient } from "@tanstack/react-query";

import { api, errorMessage } from "./api";
import { keys } from "./queries";
import { toast } from "./toast";

/** Whether worki keeps the Mac awake while agent sessions are open, and a way to change it. */
export function useKeepAwake(): [boolean, (on: boolean) => void] {
  const qc = useQueryClient();
  const { data: on = false } = useQuery({ queryKey: keys.keepAwake, queryFn: api.keepAwake });
  const setOn = (next: boolean) => {
    qc.setQueryData(keys.keepAwake, next);
    api.setKeepAwake(next).catch((e) => {
      qc.setQueryData(keys.keepAwake, !next);
      toast(`Couldn't change keep awake: ${errorMessage(e)}`);
    });
  };
  return [on, setOn];
}
