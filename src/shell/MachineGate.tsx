import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { desktop } from "../shared/api";
import { MachineContext, useMachineRegistry } from "../state/machine";
import { MachineSetup } from "../features/MachineSetup";
import { WindowTitleBar } from "./WindowTitleBar";

/**
 * Gate global do Machine Registry. Enquanto este computador não está cadastrado,
 * o App (rotas, atalhos, paleta, carregamentos) nem é montado: não há link, hash ou
 * atalho que chegue a um módulo. O backend recusa os mesmos comandos por conta própria.
 */
export function MachineGate({ children }: { children: ReactNode }) {
  const registry = useMachineRegistry();
  const wasLocked = useRef(false);
  const registered = registry.phase === "registered";

  useEffect(() => {
    if (!registered) wasLocked.current = registry.phase !== "loading";
    // Recém-cadastrado: o ambiente abre no Dashboard da máquina.
    else if (wasLocked.current) {
      wasLocked.current = false;
      window.location.hash = "dashboard";
    }
  }, [registered, registry.phase]);

  if (registered)
    return <MachineContext.Provider value={registry}>{children}</MachineContext.Provider>;
  if (registry.phase === "loading" && !registry.error)
    return (
      <>
        {desktop && <WindowTitleBar />}
        <div className="machine-splash" role="status">Identificando este computador…</div>
      </>
    );
  return (
    <>
      {desktop && <WindowTitleBar />}
      <MachineSetup registry={registry} />
    </>
  );
}
