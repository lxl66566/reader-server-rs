/* @refresh reload */
import { render } from 'solid-js/web';
import 'virtual:uno.css';
import './index.css';
import App from './App';
import { initTheme } from './theme';

const root = document.querySelector('#root');

if (!(root instanceof HTMLElement)) {
  throw new TypeError(
    'Root element not found. Did you forget to add it to your index.html? Or maybe the id attribute got misspelled?',
  );
}

initTheme();

render(() => <App />, root);
