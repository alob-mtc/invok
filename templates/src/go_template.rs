// TEMPLATE
pub const MAIN_TEMPLATE: &str = r#"
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
"#;

pub const ROUTES_TEMPLATE: &str = r#"
package main

import (
    "net/http"
)

// Handler for the "/{{ROUTE}}" endpoint.
func {{HANDLER}}(w http.ResponseWriter, r *http.Request) {
    // You can access query params via r.URL.Query().
    // For example:
    // query := r.URL.Query()
    // name := query.Get("name")

	w.WriteHeader(http.StatusOK)
	w.Write([]byte("Hello World!"))
}
"#;

pub const DOCKERFILE_TEMPLATE: &str = r#"
# Stage 1: Build Stage
FROM golang:1.23 as builder

# Set the working directory inside the container
WORKDIR /app

# Copy the specific function package into the container's workspace
# Replace {{FUNCTION}} with the actual function directory or file
COPY . .

# Initialize the Go module (if not already initialized)
RUN go mod init serverless-function

# Download dependencies early to leverage Docker cache
RUN go mod tidy

# Build the Go app
RUN CGO_ENABLED=0 GOOS=linux go build -o main .

# Stage 2: Runtime Stage
FROM gcr.io/distroless/static-debian12

# Set the working directory inside the container
WORKDIR /app

# Copy the compiled binary from the builder stage
COPY --from=builder /app/main .

# Expose port 8080
EXPOSE 8080

# Set environment variables (replace with actual environment configurations)
{{ENV}}

# Command to run the application
CMD ["./main"]
"#;

pub const FUNCTION_MODULE_TEMPLATE: &str = r#"
module serverless-function

go 1.23
"#;
