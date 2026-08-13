// Auth state: user JWT + profile, persisted token in localStorage.

import { useNavigate } from '@solidjs/router';
import {
  createContext,
  createResource,
  createSignal,
  useContext,
  type ParentComponent,
} from 'solid-js';

import { authApi, setToken, setUnauthorizedHandler } from '../api';
import type { UserInfo } from '../api/types';

interface AuthContextValue {
  isLoggedIn: () => boolean;
  login: (username: string, password: string) => Promise<void>;
  logout: () => void;
  refetchUser: () => void;
  register: (
    username: string,
    password: string,
    inviteCode?: string,
  ) => Promise<void>;
  user: () => undefined | UserInfo;
}

const AuthContext = createContext<AuthContextValue>();

export const AuthProvider: ParentComponent = props => {
  const [hasToken, setHasToken] = createSignalHasToken();
  const [user, { refetch }] = createResource(hasToken, () =>
    authApi.userInfo(),
  );

  const logout = () => {
    setToken('user', null);
    setHasToken(false);
  };

  // 401 anywhere means the token is dead: drop it and force re-login.
  setUnauthorizedHandler(logout);

  const afterAuth = (token: string) => {
    setToken('user', token);
    setHasToken(true);
    void refetch();
  };

  const value: AuthContextValue = {
    isLoggedIn: () => hasToken(),
    login: async (username, password) => {
      const res = await authApi.login(username, password);
      afterAuth(res.token);
    },
    logout,
    refetchUser: () => void refetch(),
    register: async (username, password, inviteCode) => {
      const res = await authApi.register(username, password, inviteCode);
      afterAuth(res.token);
    },
    user: () => user(),
  };

  return (
    <AuthContext.Provider value={value}>{props.children}</AuthContext.Provider>
  );
};

export function useAuth(): AuthContextValue {
  const ctx = useContext(AuthContext);
  if (!ctx) throw new Error('useAuth must be used within AuthProvider');
  return ctx;
}

// Route guard: redirects to /login when there is no user token.
export function useRequireAuth(): void {
  const auth = useAuth();
  const navigate = useNavigate();
  if (!auth.isLoggedIn()) {
    navigate('/login', { replace: true });
  }
}

function createSignalHasToken(): [() => boolean, (value: boolean) => void] {
  const [hasToken, setHasToken] = createSignal(
    localStorage.getItem('rs_token') !== null,
  );
  return [hasToken, setHasToken];
}
