module Types exposing
    ( Account
    , AccountLogin
    , AccountSetupResult
    , BootstrapData
    , ComposeState(..)
    , CredentialPreview
    , Folder
    , LotusEvent
    , MailboxSnapshot
    , MessageDetail
    , MessageSummary
    , MessageUpdate
    , Model
    , Msg(..)
    , ProviderOption
    , RequestKind(..)
    , SetupState(..)
    , SyncProgress
    , SyncStatus
    , detailToSummary
    , emptySync
    , initialModel
    , unifiedInboxId
    )

import Dict exposing (Dict)
import Json.Encode as Encode
import Time



-- MAILBOX


type alias Account =
    { id : String
    , displayName : String
    , emailAddress : String

    -- Display label, for example "Gmail".
    , provider : String

    -- Machine discriminant, for example "gmail".
    , providerKind : String
    , accent : String
    , connected : Bool
    }


type alias Folder =
    { id : String
    , accountId : String
    , name : String
    , role : String
    , unreadCount : Int
    }


{-| A message belongs to many folders at once: a Gmail message carries INBOX and
a user label simultaneously.
-}
type alias MessageSummary =
    { id : String
    , accountId : String
    , folderIds : List String
    , senderName : String
    , senderEmail : String
    , subject : String
    , snippet : String

    -- Parsed from ISO-8601 on the wire. Formatted for display at render time.
    , receivedAt : Time.Posix
    , unread : Bool
    , starred : Bool
    , labels : List String
    }


type alias MessageDetail =
    { id : String
    , accountId : String
    , folderIds : List String
    , senderName : String
    , senderEmail : String
    , subject : String
    , snippet : String
    , receivedAt : Time.Posix
    , unread : Bool
    , starred : Bool
    , labels : List String
    , to : List String
    , cc : List String
    , replyTo : List String
    , bodyParagraphs : List String
    }


type alias SyncStatus =
    { state : String

    -- Nothing when no sync has run yet.
    , lastChecked : Maybe Time.Posix
    , detail : String
    }



-- SETUP


type alias ProviderOption =
    { provider : String
    , displayName : String
    , description : String

    -- True for Gmail: the flow opens the system browser instead of asking for
    -- an email address.
    , browserLogin : Bool
    }


type alias AccountLogin =
    { provider : String
    , loginUrl : String

    -- Correlation id for browser logins. Events carrying a different state
    -- belong to another attempt and are discarded.
    , loginState : String
    , expiresAt : String
    , scopes : List String
    , browserLogin : Bool
    }


type alias CredentialPreview =
    { accountId : String
    , provider : String
    , accessTokenTail : String
    , refreshTokenTail : String
    , expiresAt : String
    }



-- WIRE PAYLOADS


type alias BootstrapData =
    { providerOptions : List ProviderOption
    , accounts : List Account
    , folders : List Folder
    , messages : List MessageSummary
    , selectedFolderId : String
    , selectedMessageId : Maybe String
    , selectedMessage : Maybe MessageDetail
    , syncStatus : SyncStatus
    }


type alias MailboxSnapshot =
    { folderId : String
    , folders : List Folder
    , messages : List MessageSummary
    , selectedMessageId : Maybe String
    , selectedMessage : Maybe MessageDetail
    , syncStatus : SyncStatus
    }


type alias MessageUpdate =
    { folders : List Folder
    , message : MessageDetail
    , syncStatus : SyncStatus
    }


type alias AccountSetupResult =
    { bootstrap : BootstrapData
    , credential : CredentialPreview
    }


type alias SyncProgress =
    { imported : Int
    , total : Maybe Int
    , detail : String
    }


{-| Pushed from Rust over the `eventIn` port. Everything except a plain progress
tick carries the `loginState` it belongs to.
-}
type alias LotusEvent =
    { kind : String
    , loginState : Maybe String
    , message : Maybe String
    , progress : Maybe SyncProgress
    , bootstrap : Maybe BootstrapData
    }



-- STATE


type SetupState
    = SetupClosed
    | ChoosingProvider
    | StartingLogin String
      -- Mock providers only: the typed-email form.
    | MockLogin AccountLogin
      -- Browser flow. The String is the loginState this view is waiting on.
    | WaitingForCallback String String
    | LoginFailed String
    | LoadingMailbox String


type ComposeState
    = ComposeIdle
    | ComposeSending
    | ComposeSent
    | ComposeFailed String


type RequestKind
    = BootstrapRequest
    | BeginLoginRequest
    | CompleteLoginRequest
    | DisconnectRequest
    | SelectFolderRequest
    | SelectMessageRequest
    | SearchRequest
    | MarkReadRequest
    | ArchiveRequest
    | RefreshRequest


type alias Model =
    { providerOptions : List ProviderOption
    , accounts : List Account
    , folders : List Folder
    , collapsedAccounts : Dict String Bool
    , messages : List MessageSummary
    , selectedFolderId : Maybe String
    , selectedMessageId : Maybe String
    , selectedMessage : Maybe MessageDetail
    , syncStatus : SyncStatus
    , search : String
    , booted : Bool
    , loading : Bool
    , error : Maybe String
    , pending : Dict Int RequestKind
    , nextRequestId : Int
    , setup : SetupState
    , loginEmail : String
    , credentialPreview : Maybe CredentialPreview
    , composeOpen : Bool
    , composeTo : String
    , composeSubject : String
    , composeBody : String
    , composeState : ComposeState

    -- Threaded from Task.here so timestamps render in local time.
    , timeZone : Time.Zone

    -- Reference instant for "today" and "yesterday". Ticked once a minute so a
    -- window left open overnight does not keep calling yesterday today.
    , now : Time.Posix
    , syncProgress : Maybe SyncProgress
    }


type Msg
    = GotCommand Encode.Value
    | GotEvent Encode.Value
    | GotTimeZone Time.Zone
    | Tick Time.Posix
    | SelectFolder String
    | SelectMessage String
    | SearchInput String
    | RunSearch
    | Refresh
    | ToggleSelectedRead
    | ArchiveSelected
    | StartAddAccount
    | CancelAddAccount
    | BackToProviders
    | BeginProviderLogin String
    | LoginEmailInput String
    | AuthorizeAccount
    | DisconnectAccount String
    | ToggleAccountFolders String
    | OpenCompose
    | CloseCompose
    | ComposeTo String
    | ComposeSubject String
    | ComposeBody String
    | SendCompose
    | NoOp


emptySync : SyncStatus
emptySync =
    { state = "Starting"
    , lastChecked = Nothing
    , detail = "Opening local mailbox"
    }


initialModel : Model
initialModel =
    { providerOptions = []
    , accounts = []
    , folders = []
    , collapsedAccounts = Dict.empty
    , messages = []
    , selectedFolderId = Nothing
    , selectedMessageId = Nothing
    , selectedMessage = Nothing
    , syncStatus = emptySync
    , search = ""
    , booted = False
    , loading = True
    , error = Nothing
    , pending = Dict.singleton 1 BootstrapRequest
    , nextRequestId = 2
    , setup = SetupClosed
    , loginEmail = ""
    , credentialPreview = Nothing
    , composeOpen = False
    , composeTo = ""
    , composeSubject = ""
    , composeBody = ""
    , composeState = ComposeIdle
    , timeZone = Time.utc
    , now = Time.millisToPosix 0
    , syncProgress = Nothing
    }


unifiedInboxId : String
unifiedInboxId =
    "unified-inbox"


detailToSummary : MessageDetail -> MessageSummary
detailToSummary message =
    { id = message.id
    , accountId = message.accountId
    , folderIds = message.folderIds
    , senderName = message.senderName
    , senderEmail = message.senderEmail
    , subject = message.subject
    , snippet = message.snippet
    , receivedAt = message.receivedAt
    , unread = message.unread
    , starred = message.starred
    , labels = message.labels
    }
