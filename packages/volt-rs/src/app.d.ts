import type { Component, ComponentType } from "react";
import type { NextRouter } from "./router";

export interface AppProps<P = any> {
  Component: ComponentType<P>;
  pageProps: P;
  router: NextRouter;
}

export default class App<P = {}> extends Component<AppProps & P> {
  render(): JSX.Element;
}
export { App };
