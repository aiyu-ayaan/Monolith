import { MonolithClient, createClient } from "./index.js";
import type { ClientOptions } from "./http/types.js";

export * from "./index.js";

/**
 * Creates a Monolith client configured for React Native environments.
 */
export function createReactNativeClient(options: ClientOptions): MonolithClient {
  return createClient(options);
}

