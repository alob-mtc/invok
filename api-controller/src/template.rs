// TEMPLATE
pub const MAIN_TEMPLATE: &str = r#"
package main

import (
	"fmt"
	"log"
	"net/http"
	"serverless-function/functions"
)

func main() {
	// Register the "/{{ROUTE}}" endpoint with the helloHandler.
	http.HandleFunc("/{{ROUTE}}", functions.{{HANDLER}})

	// Start the server on port 8080.
	fmt.Println("Server is running on port 8080...")
	log.Fatal(http.ListenAndServe(":8080", nil))
}
"#;
pub const DOCKERFILE_TEMPLATE: &str = r#"
# Use the official Golang image as a base image
FROM golang:1.18

# Set the working directory inside the container
WORKDIR /app

# Copy the local package files to the container's workspace
COPY ./temp/{{FUNCTION}} .

# Copy go mod and sum files
RUN go mod init serverless-function

# Download and install any required third-party dependencies
RUN go mod download

# Build the Go app
RUN go build -o main .

# Expose port 8080 to the outside world
EXPOSE 8080

# Command to run the executable
CMD ["./main"]
"#;
