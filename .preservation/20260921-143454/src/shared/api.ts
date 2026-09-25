import { invoke, isTauri } from "@tauri-apps/api/core";
export const desktop = isTauri();
export async function api<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  if (!desktop)
    throw new Error(
      "Recurso nativo indisponível na prévia web. Abra o aplicativo com npm run tauri dev.",
    );
  return invoke<T>(command, args);
}
export function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
