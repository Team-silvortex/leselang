export interface Channel<T> { onmessage: (message: T) => void }
export interface TauriApi {
  core: {
    invoke<T>(command: string, arguments_?: Record<string, unknown>): Promise<T>;
    Channel: new <T>() => Channel<T>;
  };
}
declare global {
  interface Window { __TAURI__?: TauriApi }
}
