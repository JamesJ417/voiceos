FROM python:3.12-alpine

RUN addgroup -S voiceos && adduser -S -G voiceos voiceos
WORKDIR /app
COPY services /app/services
COPY contracts /app/contracts

USER voiceos
EXPOSE 8787
ENTRYPOINT ["python", "-m", "services.gateway.server"]
CMD ["--host", "0.0.0.0", "--port", "8787"]
