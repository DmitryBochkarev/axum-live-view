import morphdom from "morphdom"

export class LiveView {
  private options: LiveViewOptions

  constructor() {
    this.options = {
      debug: false,
    }
    connect(this.options)
  }

  enableDebug() {
    this.options.debug = true
  }

  disableDebug() {
    this.options.debug = false
  }
}

interface LiveViewOptions {
  debug: boolean,
}

interface State {
  viewState?: Template;
  sseId?: string;
  longPollId?: string;
}

// ---------------------------------------------------------------------------
// Transport abstraction
// ---------------------------------------------------------------------------

interface Transport {
  send(msg: MessageToView): void;
  disconnect(): void;
}

// ---------------------------------------------------------------------------
// Connection entry point
// ---------------------------------------------------------------------------

/// Reads <meta name="live-view-transport" content="..."> from the page head.
/// Supported values: "sse", "websocket" (or "ws"), "longpoll", "auto" (default).
function detectTransportPreference(): "sse" | "websocket" | "longpoll" | "auto" {
  const meta = document.querySelector("meta[name='live-view-transport']")
  if (!meta) {
    return "auto"
  }
  const content = meta.getAttribute("content")?.toLowerCase() || ""
  if (content === "sse" || content === "server-sent-events") {
    return "sse"
  }
  if (content === "websocket" || content === "ws") {
    return "websocket"
  }
  if (content === "longpoll" || content === "long-poll" || content == "lp") {
    return "longpoll"
  }
  console.warn(
    `axum-live-view: unrecognized transport preference "${content}" in <meta name="live-view-transport">. ` +
    `Supported values: "sse", "websocket" (or "ws"), "longpoll" (or "lp"). Falling back to "auto".`,
  )
  return "auto"
}

function connect(options: LiveViewOptions) {
  const container = document.getElementById("live-view-container")
  if (container === null) {
    return
  }

  const preference = detectTransportPreference()

  if (preference === "sse") {
    // SSE only — skip WebSocket entirely
    connectSse(options)
  } else if (preference === "websocket") {
    // WebSocket only — no fallback
    connectWs(options, () => {
      // WebSocket failed and no fallback is allowed; retry after delay
      scheduleReconnect(options)
    })
  } else if (preference === "longpoll") {
    // Long-poll only — skip WS and SSE entirely
    connectLongPoll(options)
  } else {
    // Auto: try WebSocket first, fall back to SSE, then long-poll
    connectWs(options, () => {
      connectSseWithFallback(options, () => {
        connectLongPoll(options)
      })
    })
  }
}

// ---------------------------------------------------------------------------
// WebSocket transport
// ---------------------------------------------------------------------------

var reconnectTimeoutId: number | null = null

function scheduleReconnect(options: LiveViewOptions) {
  if (reconnectTimeoutId !== null) {
    clearTimeout(reconnectTimeoutId)
  }
  reconnectTimeoutId = setTimeout(() => {
    reconnectTimeoutId = null
    connect(options)
  }, 1000)
}

function connectWs(options: LiveViewOptions, onFallback: () => void) {
  var proto: string
  if (location.protocol.indexOf("https") === -1) {
    proto = "ws"
  } else {
    proto = "wss"
  }

  const url = `${proto}://${window.location.host}${window.location.pathname}`
  const socket = new WebSocket(url)
  var state: State = {}
  var didOpen = false
  var fallbackCalled = false
  var transport: Transport | null = null
  var heartbeatInterval: number | null = null

  function clearHeartbeat() {
    if (heartbeatInterval !== null) {
      clearInterval(heartbeatInterval)
      heartbeatInterval = null
    }
  }

  socket.addEventListener("open", () => {
    didOpen = true
    // WebSocket connected successfully
    const container = document.getElementById("live-view-container")
    if (container) {
      container.setAttribute("data-lv-connected", "true")
    }

    transport = {
      send(msg: MessageToView): void {
        socket.send(JSON.stringify(msg))
      },
      disconnect(): void {
        socket.close()
      }
    }

    // Heartbeat
    heartbeatInterval = setInterval(() => {
      const msg: MessageToView = { t: "h" }
      if (options.debug) {
        console.time(pingTimeLabel)
      }
      transport!.send(msg)
    }, 30 * 1000)

    // Bind initial events
    bindInitialEvents(transport, options)
  })

  socket.addEventListener("message", (event) => {
    const msg: MessageFromView = JSON.parse(event.data)
    handleServerMessage(transport, msg, state, options)
  })

  socket.addEventListener("close", () => {
    clearHeartbeat()
    const container = document.getElementById("live-view-container")
    if (container) {
      container.removeAttribute("data-lv-connected")
    }
    if (!didOpen && !fallbackCalled) {
      fallbackCalled = true
      // WS failed at connection time — fall back to SSE
      onFallback()
    } else {
      // Reconnection after a successful session or after fallback failed
      scheduleReconnect(options)
    }
  })

  socket.addEventListener("error", () => {
    if (!fallbackCalled && !didOpen) {
      fallbackCalled = true
      onFallback()
    }
  })
}

// ---------------------------------------------------------------------------
// SSE transport (fallback)
// ---------------------------------------------------------------------------

var sseHeartbeatInterval: number | null = null

function connectSse(options: LiveViewOptions) {
  connectSseInner(options, () => {
    // SSE failed — reconnect after a delay (retry WS first)
    scheduleReconnect(options)
  })
}

function connectSseWithFallback(options: LiveViewOptions, onFallback: () => void) {
  connectSseInner(options, onFallback)
}

function connectSseInner(options: LiveViewOptions, onError: () => void) {
  const url = `${window.location.pathname}`
  var eventSource: EventSource
  var state: State = {}
  var transport: Transport | null = null
  var errorCalled = false

  try {
    eventSource = new EventSource(url)
  } catch (_e) {
    // EventSource constructor can throw in some environments
    if (!errorCalled) {
      errorCalled = true
      onError()
    }
    return
  }

  eventSource.addEventListener("message", (event) => {
    const msg: MessageFromView = JSON.parse(event.data)

    // First message from SSE contains the connection ID
    if (msg.t === "i" && "id" in msg) {
      state.sseId = (msg as any).id
      transport = createSseTransport(state.sseId!)

      // Mark the container as connected so tests can wait for it
      const container = document.getElementById("live-view-container")
      if (container) {
        container.setAttribute("data-lv-connected", "true")
      }

      // Bind initial events now that transport is ready
      bindInitialEvents(transport, options)

      // Clear old heartbeat interval and start a new one
      if (sseHeartbeatInterval !== null) {
        clearInterval(sseHeartbeatInterval)
      }
      sseHeartbeatInterval = setInterval(() => {
        if (options.debug) {
          console.time(pingTimeLabel)
        }
        sendSseEvent(state.sseId!, { t: "h" })
      }, 30 * 1000)
    }

    handleServerMessage(transport, msg, state, options)
  })

  eventSource.addEventListener("error", () => {
    eventSource.close()
    // Remove connected marker
    const container = document.getElementById("live-view-container")
    if (container) {
      container.removeAttribute("data-lv-connected")
    }
    // Clear heartbeat to avoid stale POSTs with old connection ID
    if (sseHeartbeatInterval !== null) {
      clearInterval(sseHeartbeatInterval)
      sseHeartbeatInterval = null
    }
    // If we never received the initial message (connection failed),
    // call onError to fall back to next transport.
    if (!state.sseId && !errorCalled) {
      errorCalled = true
      onError()
      return
    }
    // Reconnect after a delay
    scheduleReconnect(options)
  })
}

function createSseTransport(sseId: string): Transport {
  return {
    send(msg: MessageToView): void {
      sendSseEvent(sseId, msg)
    },
    disconnect(): void {
      // EventSource auto-disconnects
    }
  }
}

function sendSseEvent(sseId: string, msg: MessageToView): void {
  fetch(`${window.location.pathname}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "x-live-view-event": "true",
      "x-live-view-id": sseId,
    },
    body: JSON.stringify(msg),
  }).catch((err) => {
    console.error("Failed to send SSE event:", err)
  })
}

// ---------------------------------------------------------------------------
// Long-Poll transport (last-resort fallback)
// ---------------------------------------------------------------------------

var longPollHeartbeatInterval: number | null = null

function connectLongPoll(options: LiveViewOptions) {
  const url = `${window.location.pathname}`
  var state: State = {}
  var transport: Transport | null = null

  // Initial poll: opens the view on the server and gets the initial
  // render + connection ID.
  fetch(url, {
    headers: {
      "Accept": "text/x-live-view-longpoll",
    },
  })
    .then((response) => {
      if (!response.ok) {
        throw new Error(`long-poll initial request failed: ${response.status}`)
      }
      return response.json()
    })
    .then((messages: MessageFromView[]) => {
      if (!messages || messages.length === 0) {
        throw new Error("empty initial long-poll response")
      }

      // Find the initial render message to get the connection ID
      for (const msg of messages) {
        if (msg.t === "i" && "id" in msg) {
          state.longPollId = (msg as any).id
          transport = createLongPollTransport(state.longPollId!)

          const container = document.getElementById("live-view-container")
          if (container) {
            container.setAttribute("data-lv-connected", "true")
          }

          // Bind initial events now that transport is ready
          bindInitialEvents(transport, options)

          // Heartbeat
          if (longPollHeartbeatInterval !== null) {
            clearInterval(longPollHeartbeatInterval)
          }
          longPollHeartbeatInterval = setInterval(() => {
            if (options.debug) {
              console.time(pingTimeLabel)
            }
            sendLongPollEvent(state.longPollId!, { t: "h" })
          }, 30 * 1000)
        }

        handleServerMessage(transport, msg, state, options)
      }

      // Start the long-poll loop for subsequent updates
      startLongPollLoop(options, state)
    })
    .catch((err) => {
      console.error("Long-poll connection failed:", err)
      const container = document.getElementById("live-view-container")
      if (container) {
        container.removeAttribute("data-lv-connected")
      }
      // Retry after a delay
      scheduleReconnect(options)
    })
}

function startLongPollLoop(options: LiveViewOptions, state: State) {
  if (!state.longPollId) {
    return
  }

  const url = `${window.location.pathname}`

  const doPoll = () => {
    if (!state.longPollId) {
      return
    }

    fetch(url, {
      headers: {
        "Accept": "text/x-live-view-longpoll",
        "x-live-view-id": state.longPollId,
      },
    })
      .then((response) => {
        if (!response.ok) {
          if (response.status === 410) {
            // Connection expired — reconnect from scratch
            throw new Error("connection expired")
          }
          throw new Error(`long-poll request failed: ${response.status}`)
        }
        return response.json()
      })
      .then((messages: MessageFromView[]) => {
        if (!messages || messages.length === 0) {
          // Empty response means timeout; loop continues
          return
        }

        const transport = createLongPollTransport(state.longPollId!)
        for (const msg of messages) {
          handleServerMessage(transport, msg, state, options)
        }

        // Immediately start next poll
        doPoll()
      })
      .catch((err) => {
        console.error("Long-poll error:", err)
        const container = document.getElementById("live-view-container")
        if (container) {
          container.removeAttribute("data-lv-connected")
        }
        if (longPollHeartbeatInterval !== null) {
          clearInterval(longPollHeartbeatInterval)
          longPollHeartbeatInterval = null
        }
        // Reconnect after a delay
        scheduleReconnect(options)
      })
  }

  doPoll()
}

function createLongPollTransport(longPollId: string): Transport {
  return {
    send(msg: MessageToView): void {
      sendLongPollEvent(longPollId, msg)
    },
    disconnect(): void {
      // No persistent connection to close; the next poll will 410
    },
  }
}

function sendLongPollEvent(longPollId: string, msg: MessageToView): void {
  fetch(`${window.location.pathname}`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "x-live-view-event": "true",
      "x-live-view-id": longPollId,
    },
    body: JSON.stringify(msg),
  }).catch((err) => {
    console.error("Failed to send long-poll event:", err)
  })
}

// ---------------------------------------------------------------------------
// Shared server message handling
// ---------------------------------------------------------------------------

const pingTimeLabel = "ping"

function handleServerMessage(
  transport: Transport | null,
  msg: MessageFromView,
  state: State,
  options: LiveViewOptions,
) {
  if (msg.t === "i") {
    state.viewState = msg.d
    // DOM is already populated from the initial HTTP response.
    // bindInitialEvents is called by each transport at connection time
    // (WS: in open handler, SSE/long-poll: after receiving connection ID).

  } else if (msg.t === "r") {
    if (!state.viewState) { return }
    if (!msg.d) { return }
    patchTemplate(state.viewState, msg.d)
    if (transport) {
      updateDomFromState(transport, state, options)
    }

  } else if (msg.t === "j") {
    for (const jsCommand of msg.d) {
      handleJsCommand(jsCommand)
    }

  } else if (msg.t === "h") {
    // do nothing...
    if (options.debug) {
      console.timeEnd(pingTimeLabel)
    }

  } else {
    const _: never = msg
  }
}

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

type MessageFromView = InitialRender | Render | JsCommands | HealthPong

interface Template {
  f: string[],
  d?: {
    [index: string]: TemplateDynamic
  },
}

type TemplateDynamic = string | Template | TemplateLoop

interface TemplateLoop {
  f: string[],
  b: {
    [index: string]: { [index: string]: TemplateDynamic }
  }
}

interface TemplateDiff {
  f?: string[],
  d?: {
    [index: string]: TemplateDiffDynamic | null
  }
}

type TemplateDiffDynamic = string | TemplateDiff | TemplateDiffLoop

interface TemplateDiffLoop {
  f: string[],
  b: {
    [index: string]: { [index: string]: TemplateDiffDynamic  } | null
  }
}

type InitialRender = {
  t: "i",
  d: Template,
  id?: string,
}

type Render = {
  t: "r",
  d: TemplateDiff | null,
}

type JsCommands = {
  t: "j",
  d: JsCommand[],
}

type HealthPong = { t: "h" }

type MessageToView =
  Click
  | Form
  | Input
  | Key
  | WindowFocus
  | WindowBlur
  | Mouse
  | Scroll
  | HealthPing

interface HealthPing { t: "h" }

interface Click { t: "click", m: string | JSON }

interface WindowFocus { t: "window_focus", m: string | JSON }
interface WindowBlur { t: "window_blur", m: string | JSON }

interface Scroll {
  t: "scroll",
  m: string | JSON,
  d: {
    sx: number,
    sy: number,
  }
}

interface Form {
  t: "form",
  m: string | JSON,
  d: {
    q: string
  }
}

interface Input {
  t: "input",
  m: string | JSON,
  d: {
    v: InputValue
  }
}

interface Key {
  t: "key",
  m: string | JSON,
  d: KeyData,
}

interface KeyData {
  k: string,
  kc: string,
  a: boolean,
  c: boolean,
  s: boolean,
  me: boolean,
}

interface Mouse {
  t: "mouse",
  m: string | JSON,
  d: MouseData,
}

interface MouseData {
  cx: number,
  cy: number,
  px: number,
  py: number,
  ox: number,
  oy: number,
  mx: number,
  my: number,
  sx: number,
  sy: number,
}

type InputValue = string | string[] | boolean

// ---------------------------------------------------------------------------
// Event bindings
// ---------------------------------------------------------------------------

const axm = {
  click: "axm-click",
  input: "axm-input",
  change: "axm-change",
  submit: "axm-submit",
  focus: "axm-focus",
  blur: "axm-blur",
  keydown: "axm-keydown",
  keyup: "axm-keyup",
  mouseenter: "axm-mouseenter",
  mouseover: "axm-mouseover",
  mouseleave: "axm-mouseleave",
  mouseout: "axm-mouseout",
  mousemove: "axm-mousemove",
}

const axm_window = {
  keydown: "axm-window-keydown",
  keyup: "axm-window-keyup",
  focus: "axm-window-focus",
  blur: "axm-window-blur",
  scroll: "axm-scroll",
}

function bindInitialEvents(transport: Transport, options: LiveViewOptions) {
  const attrs = Object.values(axm).map((attr) => `[${attr}]`).join(", ")

  document.querySelectorAll(attrs).forEach((element) => {
    addEventListeners(transport, element, options)
  })
}

function addEventListeners(
  transport: Transport,
  element: Element,
  options: LiveViewOptions,
) {
  if (element.hasAttribute(axm.click)) {
    on(transport, options, element, element, "click", axm.click, (msg) => ({ t: "click", m: msg }))
  }

  if (
    element instanceof HTMLInputElement ||
      element instanceof HTMLTextAreaElement ||
      element instanceof HTMLSelectElement
  ) {
    if (element.hasAttribute(axm.input)) {
      on(transport, options, element, element, "input", axm.input, (msg) => {
        const value = inputValue(element)
        return { t: "input", m: msg, d: { v: value } }
      })
    }

    if (element.hasAttribute(axm.change)) {
      on(transport, options, element, element, "change", axm.change, (msg) => {
        const value = inputValue(element)
        return { t: "input", m: msg, d: { v: value } }
      })
    }

    if (element.hasAttribute(axm.focus)) {
      on(transport, options, element, element, "focus", axm.focus, (msg) => {
        const value = inputValue(element)
        return { t: "input", m: msg, d: { v: value } }
      })
    }

    if (element.hasAttribute(axm.blur)) {
      on(transport, options, element, element, "blur", axm.blur, (msg) => {
        const value = inputValue(element)
        return { t: "input", m: msg, d: { v: value } }
      })
    }
  }

  if (element instanceof HTMLFormElement) {
    if (element.hasAttribute(axm.change)) {
      on(transport, options, element, element, "change", axm.change, (msg) => {
        const form = new FormData(element) as any
        const query = new URLSearchParams(form).toString()
        return { t: "form", m: msg, d: { q: query } }
      })
    }

    if (element.hasAttribute(axm.submit)) {
      on(transport, options, element, element, "submit", axm.submit, (msg) => {
        const form = new FormData(element) as any
        const query = new URLSearchParams(form).toString()
        return { t: "form", m: msg, d: { q: query } }
      })
    }
  }

  [
    ["mouseenter", axm.mouseenter],
    ["mouseover", axm.mouseover],
    ["mouseleave", axm.mouseleave],
    ["mouseout", axm.mouseout],
    ["mousemove", axm.mousemove],
  ].forEach(([event, axm]) => {
    if (!event) { return }
    if (!axm) { return }

    if (element.hasAttribute(axm)) {
      on(transport, options, element, element, event, axm, (msg, event) => {
        if (event instanceof MouseEvent) {
          const data: MouseData = {
            cx: event.clientX,
            cy: event.clientY,
            px: event.pageX,
            py: event.pageY,
            ox: event.offsetX,
            oy: event.offsetY,
            mx: event.movementX,
            my: event.movementY,
            sx: event.screenX,
            sy: event.screenY,
          }
          return { t: "mouse", m: msg, d: data }
        } else {
          return
        }
      })
    }
  });

  [
    ["keydown", axm.keydown],
    ["keyup", axm.keyup],
  ].forEach(([event, axm]) => {
    if (!event) { return }
    if (!axm) { return }

    if (element.hasAttribute(axm)) {
      on(transport, options, element, element, event, axm, (msg, event) => {
        if (event instanceof KeyboardEvent) {
          if (
            element.hasAttribute("axm-key") &&
            element?.getAttribute("axm-key")?.toLowerCase() !== event.key.toLowerCase()
          ) {
            return;
          }

          const data: KeyData = {
            k: event.key,
            kc: event.code,
            a: event.altKey,
            c: event.ctrlKey,
            s: event.shiftKey,
            me: event.metaKey,
          }
          return { t: "key", m: msg, d: data }
        } else {
          return
        }
      })
    }
  });

}

function addDocumentEventListeners(
  transport: Transport,
  element: Element,
  options: LiveViewOptions,
) {
  [
    ["keydown", axm_window.keydown],
    ["keyup", axm_window.keyup],
  ].forEach(([event, axm]) => {
    if (!event) { return }
    if (!axm) { return }

    if (element.hasAttribute(axm)) {
      on(transport, options, element, document, event, axm, (msg, event) => {
        if (event instanceof KeyboardEvent) {
          if (
            element.hasAttribute("axm-key") &&
            element?.getAttribute("axm-key")?.toLowerCase() !== event.key.toLowerCase()
          ) {
            return;
          }

          const data: KeyData = {
            k: event.key,
            kc: event.code,
            a: event.altKey,
            c: event.ctrlKey,
            s: event.shiftKey,
            me: event.metaKey,
          }
          return { t: "key", m: msg, d: data }
        } else {
          return
        }
      })
    }
  });

  if (element.hasAttribute(axm_window.focus)) {
    on(transport, options, element, document, "focus", axm_window.focus, (msg) => {
      return { t: "window_focus", m: msg }
    })
  }

  if (element.hasAttribute(axm_window.blur)) {
    on(transport, options, element, document, "blur", axm_window.blur, (msg) => {
      return { t: "window_blur", m: msg }
    })
  }

  if (element.hasAttribute(axm_window.scroll)) {
    on(transport, options, element, document, "scroll", axm_window.scroll, (msg) => {
      const data = {
        sx: window.scrollX,
        sy: window.scrollY,
      }
      return { t: "scroll", m: msg, d: data }
    })
  }
}

function on(
  transport: Transport,
  options: LiveViewOptions,
  element: Element,
  listenForEventOn: Element | typeof document,
  eventName: string,
  attr: string,
  f: (msg: string | JSON, event: Event) => MessageToView | undefined,
) {
  var callback: (event: Event) => void = delayOrThrottle(element, (event: Event) => {
    if (!(event instanceof KeyboardEvent)) {
      event.preventDefault()
    }

    const decodeMsg = msgAttr(element, attr)
    if (!decodeMsg) { return }
    const payload = f(decodeMsg, event)
    if (!payload) { return }
    transport.send(payload)
  })

  if (document === listenForEventOn) {
    documentEventListeners.push({
      event: eventName,
      callback: callback,
    })
  }

  listenForEventOn.addEventListener(eventName, callback)
}

function msgAttr(element: Element, attr: string): string | JSON | undefined {
    const value = element.getAttribute(attr)
    if (!value) { return }
    try {
      return JSON.parse(value)
    } catch {
      return value
    }
}

function delayOrThrottle<In extends unknown[]>(element: Element, f: Fn<In>): Fn<In> {
  var delayMs = numberAttr(element, "axm-debounce")
  if (delayMs) {
    return debounce(f, delayMs)
  }

  var delayMs = numberAttr(element, "axm-throttle")
  if (delayMs) {
    return throttle(f, delayMs)
  }

  return f
}

interface DocumentEventListener {
  event: string,
  callback: (event: Event) => void,
}

var documentEventListeners: DocumentEventListener[] = []

function inputValue(element: Element): InputValue {
  if (element instanceof HTMLTextAreaElement) {
    return element.value

  } else if (element instanceof HTMLInputElement) {
    if (element.getAttribute("type") === "radio" || element.getAttribute("type") === "checkbox") {
      return element.checked
    } else {
      return element.value
    }

  } else if (element instanceof HTMLSelectElement) {
    if (element.hasAttribute("multiple")) {
      return Array.from(element.selectedOptions).map((opt) => opt.value)
    } else {
      return element.value
    }

  } else {
    throw "Input has no input value"
  }
}

function numberAttr(element: Element, attr: string): number | null {
  const value = element.getAttribute(attr)
  if (value) {
    const number = parseInt(value, 10)
    if (number) {
      return number
    }
  }
  return null
}

function updateDomFromState(transport: Transport, state: State, options: LiveViewOptions) {
  if (!state.viewState) { return }
  const html = buildHtml(state.viewState)
  const container = document.querySelector("#live-view-container")
  if (!container) { return }
  patchDom(transport, container, html)

  function buildHtml(template: Template): string {
    var combined = ""
    const fixed = template.f

    fixed.forEach((value, i) => {
      combined = combined.concat(value)

      if (template.d === undefined) {
        return
      }

      const templateDyn = template.d[i]

      if (templateDyn === undefined || templateDyn === null) {
        return
      }

      if (typeof templateDyn === "string") {
        combined = combined.concat(templateDyn)

      } else if ("b" in templateDyn) {
        const fixed = templateDyn.f

        Object.values(templateDyn.b).forEach((value) => {
          const nestedTemplate = { f: fixed, d: value }
          combined = combined.concat(buildHtml(nestedTemplate))
        })

      } else {
        combined = combined.concat(buildHtml(templateDyn))
      }
    })

    return combined
  }

  function patchDom(transport: Transport, element: Element, html: string) {
    for (var i = 0; i < documentEventListeners.length; i++) {
      let e = documentEventListeners[i]
      if (!e) { continue }
      document.removeEventListener(e.event, e.callback)
      documentEventListeners.splice(i, 1);
    }

    morphdom(element, html, {
      onNodeAdded: (node) => {
        if (node instanceof Element) {
          addEventListeners(transport, node, options)
        }
        return node
      },
    })

    const attrs = Object.values(axm_window).map((attr) => `[${attr}]`).join(", ")
    document.querySelectorAll(attrs).forEach((el) => {
      addDocumentEventListeners(transport, el, options)
    })
  }
}

function patchTemplate(template: Template, diff: TemplateDiff) {
  if (diff.f) {
    template.f = diff.f
  }

  if (diff.d && diff.d !== null) {
    patchTemplateDiff(template.d || {}, diff.d)
  }

  function patchTemplateDiff(
    template: { [index: string]: TemplateDynamic },
    diff: { [index: string]: TemplateDiffDynamic | null; },
  ) {
    for (const [key, diffVal] of Object.entries(diff)) {
      if (typeof diffVal === "string") {
        template[key] = diffVal

      } else if (diffVal === null) {
        delete template[key]

      } else if (typeof diffVal === "object") {
        const current = template[key]
        if (current === undefined) {
          if ("d" in diffVal) {
            template[key] = <TemplateDynamic>diffVal
          } else if ("b" in diffVal) {
            template[key] = <TemplateLoop>diffVal
          } else if ("f" in diffVal) {
            template[key] = <TemplateDynamic>diffVal
          }
          continue
        }

        if ("d" in diffVal) {
          if (typeof current === "string") {
            template[key] = <TemplateDynamic>diffVal

          } else if ("d" in current) {
            patchTemplate(current, diffVal)

          } else if ("b" in current) {
            console.error("not implemented: b in current")

          } else {
            template[key] = <TemplateDynamic>diffVal
          }

        } else if ("b" in diffVal) {
          if (typeof current === "string") {
            template[key] = <TemplateLoop>diffVal

          } else {
            if (!("b" in current)) {
              template[key] = {
                f: current.f,
                b: <{ [index: string]: { [index: string]: TemplateDynamic } }>diffVal.b
              }
            } else {
              patchTemplateLoop(current, diffVal)
            }
          }

        } else if ("f" in diffVal) {
          if (typeof current === "string") {
            template[key] = <TemplateDynamic>diffVal

          } else if ("d" in current) {
            patchTemplate(current, diffVal)

          } else if ("b" in current) {
            console.error("not implemented: b in current, with f")

          } else {
            template[key] = <TemplateDynamic>diffVal
          }

        } else {
          console.error("unexpected diff value", diffVal)
        }

      } else {
        const _: never = diffVal
      }
    }
  }

  function patchTemplateLoop(template: TemplateLoop, diff: TemplateDiffLoop) {
    if (diff.f) {
      template.f = diff.f
    }

    if (diff.b) {
      for (const [key, diffVal] of Object.entries(diff.b)) {
        if (diffVal === null) {
          delete template.b[key]

        } else {
          const current = template.b[key]

          if (current === undefined || typeof current === "string") {
            template.b[key] = <{ [index: string]: TemplateDynamic; }>diffVal
          } else {
            patchTemplateDiff(current, diffVal)
          }
        }
      }
    }
  }
}

// ---------------------------------------------------------------------------
// JavaScript commands
// ---------------------------------------------------------------------------

interface JsCommand {
  delay_ms: number | null,
  kind: JsCommandKind,
}

type JsCommandKind =
  { t: "navigate_to", uri: string }
  | { t: "add_class", selector: string, klass: string }
  | { t: "remove_class", selector: string, klass: string }
  | { t: "toggle_class", selector: string, klass: string }
  | { t: "clear_value", selector: string }
  | { t: "set_title", title: string }
  | { t: "history_push_state", uri: string }

function handleJsCommand(cmd: JsCommand) {
  const run = () => {
    if (cmd.kind.t === "navigate_to") {
      const uri = cmd.kind.uri
      if (uri.startsWith("http")) {
        window.location.href = uri
      } else {
        window.location.pathname = uri
      }

    } else if (cmd.kind.t === "add_class") {
      const { selector, klass } = cmd.kind
      document.querySelectorAll(selector).forEach((element) => {
        element.classList.add(klass)
      })

    } else if (cmd.kind.t === "remove_class") {
      const { selector, klass } = cmd.kind
      document.querySelectorAll(selector).forEach((element) => {
        element.classList.remove(klass)
      })

    } else if (cmd.kind.t === "toggle_class") {
      const { selector, klass } = cmd.kind
      document.querySelectorAll(selector).forEach((element) => {
        element.classList.toggle(klass)
      })

    } else if (cmd.kind.t === "clear_value") {
      const { selector } = cmd.kind
      document.querySelectorAll(selector).forEach((element) => {
        if (element instanceof HTMLInputElement || element instanceof HTMLSelectElement || element instanceof HTMLTextAreaElement) {
          element.value = ""
        }
      })

    } else if (cmd.kind.t === "set_title") {
      document.title = cmd.kind.title

    } else if (cmd.kind.t === "history_push_state") {
      window.history.pushState({}, "", cmd.kind.uri);

    } else {
      const _: never = cmd.kind
    }
  }

  if (cmd.delay_ms) {
    setTimeout(run, cmd.delay_ms)
  } else {
    run()
  }
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

type Fn<
  In extends unknown[],
> = (...args: In) => void;

function debounce<In extends unknown[]>(f: Fn<In>, delayMs: number): Fn<In> {
  var timeout: number
  return (...args) => {
    if (timeout) {
      clearTimeout(timeout)
    }

    timeout = setTimeout(() => {
      f(...args)
    }, delayMs)
  }
}

function throttle<In extends unknown[]>(f: Fn<In>, delayMs: number): Fn<In> {
  var timeout: number | null
  return (...args) => {
    if (timeout) {
      return
    } else {
      f(...args)
      timeout = setTimeout(() => {
        timeout = null
      }, delayMs)
    }
  }
}
