/**
 * Type-level description of the Electron modules ow-tauri does not provide
 * (`docs/CONTRACT.md` section B.2.5, rows marked U). The member names are
 * those of Electron 42.11.4 (the version ow-electron 42.11.4 ships), so
 * ported code that still calls, for example, `Menu.setApplicationMenu(null)`
 * compiles, with the module flagged as deprecated in the editor, and throws
 * `OwTauriUnsupportedError` when it runs.
 *
 * @packageDocumentation
 */

import type { UnsupportedMethod } from '../shared/unsupported.js';

export type { UnsupportedMethod } from '../shared/unsupported.js';

/**
 * An unsupported Electron module or class: every listed member throws when
 * called, and calling or constructing the object itself throws.
 *
 * @typeParam Members - the Electron member names
 */
export type UnsupportedModule<Members extends string> = Readonly<
  Record<Members, UnsupportedMethod>
> & {
  /** Throws `OwTauriUnsupportedError`. */
  new (...args: unknown[]): never;
  /** Throws `OwTauriUnsupportedError`. */
  (...args: unknown[]): never;
};

/** Members of Electron's `Menu`. */
export type MenuMembers =
  'buildFromTemplate' | 'getApplicationMenu' | 'sendActionToFirstResponder' | 'setApplicationMenu';
/** Members of Electron's `MenuItem`. */
export type MenuItemMembers = never;
/** Members of Electron's `Tray`. */
export type TrayMembers = never;
/** Members of Electron's `Notification`. */
export type NotificationMembers =
  'getHistory' | 'handleActivation' | 'isSupported' | 'remove' | 'removeAll' | 'removeGroup';
/** Members of Electron's `session`. */
export type SessionMembers = 'defaultSession' | 'fromPartition' | 'fromPath';
/** Members of Electron's `protocol`. */
export type ProtocolMembers =
  | 'handle'
  | 'interceptBufferProtocol'
  | 'interceptFileProtocol'
  | 'interceptHttpProtocol'
  | 'interceptStreamProtocol'
  | 'interceptStringProtocol'
  | 'isProtocolHandled'
  | 'isProtocolIntercepted'
  | 'isProtocolRegistered'
  | 'registerBufferProtocol'
  | 'registerFileProtocol'
  | 'registerHttpProtocol'
  | 'registerSchemesAsPrivileged'
  | 'registerStreamProtocol'
  | 'registerStringProtocol'
  | 'unhandle'
  | 'uninterceptProtocol'
  | 'unregisterProtocol';
/** Members of Electron's `net`. */
export type NetMembers = 'WebSocket' | 'fetch' | 'isOnline' | 'online' | 'request' | 'resolveHost';
/** Members of Electron's `netLog`. */
export type NetLogMembers = 'currentlyLogging' | 'startLogging' | 'stopLogging';
/** Members of Electron's `powerMonitor`. */
export type PowerMonitorMembers =
  | 'addListener'
  | 'emit'
  | 'eventNames'
  | 'getCurrentThermalState'
  | 'getMaxListeners'
  | 'getSystemIdleState'
  | 'getSystemIdleTime'
  | 'isOnBatteryPower'
  | 'listenerCount'
  | 'listeners'
  | 'off'
  | 'on'
  | 'onBatteryPower'
  | 'once'
  | 'prependListener'
  | 'prependOnceListener'
  | 'rawListeners'
  | 'removeAllListeners'
  | 'removeListener'
  | 'setMaxListeners';
/** Members of Electron's `powerSaveBlocker`. */
export type PowerSaveBlockerMembers = 'isStarted' | 'start' | 'stop';
/** Members of Electron's `autoUpdater`. */
export type AutoUpdaterMembers =
  | 'addListener'
  | 'checkForUpdates'
  | 'emit'
  | 'eventNames'
  | 'getFeedURL'
  | 'getMaxListeners'
  | 'listenerCount'
  | 'listeners'
  | 'off'
  | 'on'
  | 'once'
  | 'prependListener'
  | 'prependOnceListener'
  | 'quitAndInstall'
  | 'rawListeners'
  | 'removeAllListeners'
  | 'removeListener'
  | 'setFeedURL'
  | 'setMaxListeners';
/** Members of Electron's `clipboard`. */
export type ClipboardMembers =
  | 'availableFormats'
  | 'clear'
  | 'has'
  | 'read'
  | 'readBookmark'
  | 'readBuffer'
  | 'readFindText'
  | 'readHTML'
  | 'readImage'
  | 'readRTF'
  | 'readText'
  | 'write'
  | 'writeBookmark'
  | 'writeBuffer'
  | 'writeFindText'
  | 'writeHTML'
  | 'writeImage'
  | 'writeRTF'
  | 'writeText';
/** Members of Electron's `nativeImage`. */
export type NativeImageMembers =
  | 'createEmpty'
  | 'createFromBitmap'
  | 'createFromBuffer'
  | 'createFromDataURL'
  | 'createFromNamedImage'
  | 'createFromPath'
  | 'createThumbnailFromPath';
/** Members of Electron's `systemPreferences`. */
export type SystemPreferencesMembers =
  | 'accessibilityDisplayShouldReduceTransparency'
  | 'addListener'
  | 'askForMediaAccess'
  | 'canPromptTouchID'
  | 'effectiveAppearance'
  | 'emit'
  | 'eventNames'
  | 'getAccentColor'
  | 'getAnimationSettings'
  | 'getColor'
  | 'getEffectiveAppearance'
  | 'getMaxListeners'
  | 'getMediaAccessStatus'
  | 'getSystemColor'
  | 'getUserDefault'
  | 'isSwipeTrackingFromScrollEventsEnabled'
  | 'isTrustedAccessibilityClient'
  | 'listenerCount'
  | 'listeners'
  | 'off'
  | 'on'
  | 'once'
  | 'postLocalNotification'
  | 'postNotification'
  | 'postWorkspaceNotification'
  | 'prependListener'
  | 'prependOnceListener'
  | 'promptTouchID'
  | 'rawListeners'
  | 'registerDefaults'
  | 'removeAllListeners'
  | 'removeListener'
  | 'removeUserDefault'
  | 'setMaxListeners'
  | 'setUserDefault'
  | 'subscribeLocalNotification'
  | 'subscribeNotification'
  | 'subscribeWorkspaceNotification'
  | 'unsubscribeLocalNotification'
  | 'unsubscribeNotification'
  | 'unsubscribeWorkspaceNotification';
/** Members of Electron's `desktopCapturer`. */
export type DesktopCapturerMembers = 'getSources';
/** Members of Electron's `webFrame`. */
export type WebFrameMembers =
  | 'clearCache'
  | 'executeJavaScript'
  | 'executeJavaScriptInIsolatedWorld'
  | 'findFrameByName'
  | 'findFrameByRoutingId'
  | 'findFrameByToken'
  | 'firstChild'
  | 'frameToken'
  | 'getFrameForSelector'
  | 'getResourceUsage'
  | 'getWordSuggestions'
  | 'getZoomFactor'
  | 'getZoomLevel'
  | 'insertCSS'
  | 'insertText'
  | 'isWordMisspelled'
  | 'nextSibling'
  | 'opener'
  | 'parent'
  | 'removeInsertedCSS'
  | 'routingId'
  | 'setIsolatedWorldInfo'
  | 'setSpellCheckProvider'
  | 'setVisualZoomLevelLimits'
  | 'setZoomFactor'
  | 'setZoomLevel'
  | 'top';
/** Members of Electron's `webFrameMain`. */
export type WebFrameMainMembers = 'fromFrameToken' | 'fromId';
/** Members of Electron's `utilityProcess`. */
export type UtilityProcessMembers = 'fork';
/** Members of Electron's `MessageChannelMain`. */
export type MessageChannelMainMembers = never;
/** Members of Electron's `BrowserView`. */
export type BrowserViewMembers = never;
/** Members of Electron's `WebContentsView`. */
export type WebContentsViewMembers = never;
/** Members of Electron's `BaseWindow`. */
export type BaseWindowMembers = 'fromId' | 'getAllWindows' | 'getFocusedWindow';
/** Members of Electron's `TouchBar`. */
export type TouchBarMembers =
  | 'TouchBarButton'
  | 'TouchBarColorPicker'
  | 'TouchBarGroup'
  | 'TouchBarLabel'
  | 'TouchBarOtherItemsProxy'
  | 'TouchBarPopover'
  | 'TouchBarScrubber'
  | 'TouchBarSegmentedControl'
  | 'TouchBarSlider'
  | 'TouchBarSpacer';
/** Members of Electron's `inAppPurchase`. */
export type InAppPurchaseMembers =
  | 'addListener'
  | 'canMakePayments'
  | 'emit'
  | 'eventNames'
  | 'finishAllTransactions'
  | 'finishTransactionByDate'
  | 'getMaxListeners'
  | 'getProducts'
  | 'getReceiptURL'
  | 'listenerCount'
  | 'listeners'
  | 'off'
  | 'on'
  | 'once'
  | 'prependListener'
  | 'prependOnceListener'
  | 'purchaseProduct'
  | 'rawListeners'
  | 'removeAllListeners'
  | 'removeListener'
  | 'restoreCompletedTransactions'
  | 'setMaxListeners';
/** Members of Electron's `pushNotifications`. */
export type PushNotificationsMembers =
  | 'addListener'
  | 'emit'
  | 'eventNames'
  | 'getMaxListeners'
  | 'listenerCount'
  | 'listeners'
  | 'off'
  | 'on'
  | 'once'
  | 'prependListener'
  | 'prependOnceListener'
  | 'rawListeners'
  | 'registerForAPNSNotifications'
  | 'removeAllListeners'
  | 'removeListener'
  | 'setMaxListeners'
  | 'unregisterForAPNSNotifications';
/** Members of Electron's `safeStorage`. */
export type SafeStorageMembers =
  | 'decryptString'
  | 'decryptStringAsync'
  | 'encryptString'
  | 'encryptStringAsync'
  | 'getSelectedStorageBackend'
  | 'isAsyncEncryptionAvailable'
  | 'isEncryptionAvailable'
  | 'setUsePlainTextEncryption';
/** Members of Electron's `contentTracing`. */
export type ContentTracingMembers =
  | 'enableHeapProfiling'
  | 'getCategories'
  | 'getTraceBufferUsage'
  | 'startRecording'
  | 'stopRecording';
