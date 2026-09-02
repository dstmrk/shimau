import * as React from "react"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"

import { Dashboard } from "@/components/dashboard"
import { LoginForm } from "@/components/login-form"
import { Toaster } from "@/components/ui/sonner"
import { ApiError, api } from "@/lib/api"
import type { Identity } from "@/lib/types"

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      // A 401 means the session is gone; retrying only delays the redirect
      // to the login screen.
      retry: (failureCount, error) =>
        !(error instanceof ApiError && error.status === 401) &&
        failureCount < 2,
    },
  },
})

export function App() {
  const [identity, setIdentity] = React.useState<Identity | null>(null)
  const [checking, setChecking] = React.useState(true)

  React.useEffect(() => {
    api
      .me()
      .then(setIdentity)
      .catch(() => setIdentity(null))
      .finally(() => setChecking(false))
  }, [])

  // A session can expire while the dashboard is open; the query layer surfaces
  // that as a 401, and this listener sends the user back to the login screen
  // instead of leaving an empty dashboard behind.
  React.useEffect(() => {
    const unsubscribe = queryClient.getQueryCache().subscribe((event) => {
      const error = event.query.state.error
      if (error instanceof ApiError && error.status === 401) {
        setIdentity(null)
      }
    })
    return unsubscribe
  }, [])

  if (checking) {
    return <div className="min-h-svh" />
  }

  return (
    <QueryClientProvider client={queryClient}>
      {identity ? (
        <Dashboard
          identity={identity}
          onSignedOut={() => {
            queryClient.clear()
            setIdentity(null)
          }}
        />
      ) : (
        <LoginForm onAuthenticated={setIdentity} />
      )}
      <Toaster />
    </QueryClientProvider>
  )
}

export default App
