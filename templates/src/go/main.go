package main

import (
    "context"
    "log"
    "net"
    "net/http"
    "os"
    "os/signal"
    "syscall"
    "time"

    "github.com/gorilla/mux"
)

func main() {
    // 1. Use environment variable or a default for the server port.
    port := os.Getenv("PORT")
    if port == "" {
        port = "8080"
    }

    // 2. Create a new router.
    r := mux.NewRouter()

    // 3. Register endpoints.
    // Register the "/{{ROUTE}}" endpoint with the {{HANDLER}}.
	r.HandleFunc("/{{ROUTE}}", {{HANDLER}})

    // 4. Create an HTTP server with timeouts & the router.
    srv := &http.Server{
        Addr:         ":" + port,
        Handler:      r,
        ReadTimeout:  5 * time.Second,  // protect against slowloris
        WriteTimeout: 10 * time.Second, // overall request timeout
        IdleTimeout:  15 * time.Second, // keep-alive time
    }

    // 5. Create a net.Listener to have more control over incoming connections.
	listener, err := net.Listen("tcp", ":"+port)
	if err != nil {
		log.Fatalf("Error starting listener: %v", err)
	}

	// 6. Start the server in a separate goroutine.
	go func() {
		log.Printf("Server is running on port %s...\n", port)
		if err := srv.Serve(listener); err != nil && err != http.ErrServerClosed {
			log.Fatalf("Server error: %v", err)
		}
	}()

	// 7. Set up channel to receive signal notifications.
	stop := make(chan os.Signal, 1)
	signal.Notify(stop, os.Interrupt, syscall.SIGTERM)

	// 8. Block until a signal is received.
	<-stop
	log.Println("Shutting down the server...")

	// 9. Stop accepting new connections immediately by closing the listener.
	if err := listener.Close(); err != nil {
		log.Printf("Error closing listener: %v", err)
	}

	// 10. Create a context with a timeout to allow active requests to finish.
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	// 11. Attempt a graceful shutdown.
	if err := srv.Shutdown(ctx); err != nil {
		log.Fatalf("Server forced to shutdown: %v", err)
	}

	log.Println("Server exited gracefully.")
}