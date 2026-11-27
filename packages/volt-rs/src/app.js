import { Component, createElement as h } from "react";

/** Default Pages Router `_app`: renders the page with its props. Extend it or write a function component. */
export default class App extends Component {
  render() {
    const { Component: Page, pageProps } = this.props;
    return h(Page, pageProps);
  }
}

export { App };
