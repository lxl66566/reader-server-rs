// Shared admin helpers: 401 detection.

import { ApiError } from '../../api/client';

// Admin tokens live in a separate namespace; any 401 means the session is dead.
export function isUnauthorized(err: unknown): boolean {
  return err instanceof ApiError && err.status === 401;
}
