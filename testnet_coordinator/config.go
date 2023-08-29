package main

import (
	"log"
	"os"

	"gopkg.in/yaml.v3"
)

type ServerConfig struct {
	ServerHost string `yaml:"host,omitempty"`
	ServerURL  string `yaml:"url,omitempty"`
}

type Config struct {
	StartBatch       uint64        `yaml:"start,omitempty"`
	EndBatch         uint64        `yaml:"end,omitempty"`
	Sampling         float32       `yaml:"sampling,omitempty"`
	GroupID          int           `yaml:"group_id,omitempty"`
	ProxyOnly        bool          `yaml:"proxy_only,omitempty"`
	ChunkURLTemplate string        `yaml:"chunkURL"`
	NotifierURL      string        `yaml:"notifierURL"`
	Server           *ServerConfig `yaml:"server,omitempty"`
}

func NewConfig() *Config {
	return &Config{
		Server: &ServerConfig{
			ServerHost: "localhost:8560",
			ServerURL:  "/api",
		},
	}
}

func (cfg *Config) LoadEnv(path string) error {
	return nil
}

func (cfg *Config) Load(path string) error {

	data, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	err = yaml.Unmarshal(data, cfg)
	if err != nil {
		return err
	}

	cfgYAML, err := yaml.Marshal(cfg)
	if err != nil {
		log.Fatal("re-marshal config file fail", err)
	} else {
		log.Printf("load config:\n%s", cfgYAML)
	}
	return nil

}
