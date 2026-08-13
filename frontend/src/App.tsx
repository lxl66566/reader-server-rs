import { Navigate, Route, Router } from '@solidjs/router';
import { lazy, type Component } from 'solid-js';
import { Toaster } from 'solid-toast';

import { Layout } from './components/Layout';
import { I18nProvider } from './i18n';
import { AuthProvider } from './stores/auth';
import { SettingsProvider } from './stores/settings';

const Home = lazy(() => import('./pages/Home'));
const Login = lazy(() => import('./pages/Login'));
const Register = lazy(() => import('./pages/Register'));
const BookDetail = lazy(() => import('./pages/BookDetail'));
const Stats = lazy(() => import('./pages/Stats'));
const Settings = lazy(() => import('./pages/Settings'));
const Reader = lazy(() => import('./pages/Reader'));
const Admin = lazy(() => import('./pages/Admin'));

const WithLayout: Component<{ page: Component }> = props => (
  <Layout>
    <props.page />
  </Layout>
);

const App: Component = () => (
  <I18nProvider>
    <AuthProvider>
      <SettingsProvider>
        <Toaster position="top-center" toastOptions={{ duration: 2500 }} />
        <Router>
          {/* reader is fullscreen: no nav layout */}
          <Route component={Reader} path="/read/:id" />
          <Route component={Admin} path="/admin" />
          <Route component={() => <WithLayout page={Home} />} path="/" />
          <Route component={() => <WithLayout page={Login} />} path="/login" />
          <Route
            component={() => <WithLayout page={Register} />}
            path="/register"
          />
          <Route
            component={() => <WithLayout page={BookDetail} />}
            path="/book/:id"
          />
          <Route component={() => <WithLayout page={Stats} />} path="/stats" />
          <Route
            component={() => <WithLayout page={Settings} />}
            path="/settings"
          />
          <Route component={() => <Navigate href="/" />} path="*" />
        </Router>
      </SettingsProvider>
    </AuthProvider>
  </I18nProvider>
);

export default App;
