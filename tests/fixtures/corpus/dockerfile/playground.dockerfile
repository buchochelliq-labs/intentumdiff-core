FROM node:18-alpine AS builder
WORKDIR /app
COPY package.json ./
RUN npm install --omit=dev
ENV APP_PORT=8080
ENV LOG_LEVEL=info
EXPOSE 9091
LABEL maintainer="platform-team"
HEALTHCHECK CMD curl --fail http://localhost:9091/health
USER svc_runner
CMD ["dist/index.js"]
