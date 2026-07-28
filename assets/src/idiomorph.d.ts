declare module "idiomorph" {
  interface IdiomorphConfig {
    morphStyle?: "innerHTML" | "outerHTML";
    ignoreActive?: boolean;
    ignoreActiveValue?: boolean;
    restoreFocus?: boolean;
    callbacks?: IdiomorphCallbacks;
    head?: IdiomorphHeadConfig;
  }

  interface IdiomorphCallbacks {
    beforeNodeAdded?: (node: Node) => boolean;
    afterNodeAdded?: (node: Node) => void;
    beforeNodeMorphed?: (oldNode: Element, newNode: Node) => boolean;
    afterNodeMorphed?: (oldNode: Element, newNode: Node) => void;
    beforeNodeRemoved?: (node: Element) => boolean;
    afterNodeRemoved?: (node: Element) => void;
    beforeAttributeUpdated?: (
      attrName: string,
      element: Element,
      changeType: "update" | "remove"
    ) => boolean;
    beforeNodePantried?: (node: Element) => boolean;
  }

  interface IdiomorphHeadConfig {
    style?: "merge" | "append" | "morph" | "none";
    block?: boolean;
    ignore?: boolean;
    shouldPreserve?: (element: Element) => boolean;
    shouldReAppend?: (element: Element) => boolean;
    shouldRemove?: (element: Element) => boolean;
    afterHeadMorphed?: (
      element: Element,
      context: { added: Node[]; kept: Element[]; removed: Element[] }
    ) => void;
  }

  interface IdiomorphStatic {
    defaults: IdiomorphConfig;
    morph(
      oldNode: Element,
      newContent: string | Element | DocumentFragment,
      config?: IdiomorphConfig
    ): void;
  }

  const Idiomorph: IdiomorphStatic;
  export = Idiomorph;
}
