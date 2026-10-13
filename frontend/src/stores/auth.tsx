// Auth state: user JWT + profile, persisted token in localStorage.

import { useNavigate } from '@solidjs/router';
import {
  createContext,
  createEffect,
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
  // source 类型参数会被推断为 true（boolean 拆入联合类型后只剩 true），
  // 所以这里显式标注回 boolean
  const [user, { refetch }] = createResource(
    hasToken,
    (tokenPresent: boolean) => {
      // refetch 会绕过 source 门控强制执行 fetcher：登出后必须短路，
      // 否则会多发一次不带 token、注定 401 的请求
      if (!tokenPresent) return Promise.resolve(undefined);
      return authApi.userInfo();
    },
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
  // 响应式监听登录态：token 过期被全局 401 处理清理后也要跳转，
  // 而不是只在挂载时检查一次
  createEffect(() => {
    if (!auth.isLoggedIn()) {
      navigate('/login', { replace: true });
    }
  });
}

function createSignalHasToken(): [() => boolean, (value: boolean) => void] {
  const [hasToken, setHasToken] = createSignal(
    localStorage.getItem('rs_token') !== null,
  );
  return [hasToken, setHasToken];
}
