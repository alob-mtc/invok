# Stage 1: Build stage
FROM node:22-alpine AS builder

# Set working directory
WORKDIR /app

# Copy package files
COPY package*.json ./

# Install dependencies (including dev dependencies for building)
RUN npm ci --only=production=false

# Copy source code
COPY . .

# Build the application
RUN npm run build

# Stage 2: Production stage
FROM node:22-alpine AS production

# Create app directory
WORKDIR /app

# Create non-root user for security
RUN addgroup -g 1001 -S nodejs && \
    adduser -S fastify -u 1001

# Copy package files
COPY package*.json ./

# Install only production dependencies
RUN npm ci --only=production && npm cache clean --force

# Copy built application from builder stage
COPY --from=builder /app/dist ./dist

# Change ownership of the app directory to the nodejs user
RUN chown -R fastify:nodejs /app

# Switch to non-root user
USER fastify

# Expose port
EXPOSE 8080

# Set environment variables (replace with actual environment configurations)
{{ENV}}

# Set environment to production
ENV NODE_ENV=production

# Start the application
CMD ["node", "dist/server.js"]