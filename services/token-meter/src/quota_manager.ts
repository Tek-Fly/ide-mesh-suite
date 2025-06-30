// Token Meter Quota Manager
// Built with prayer and excellence

import { Redis } from 'ioredis';
import { MongoClient, Db, Collection } from 'mongodb';
import * as yaml from 'js-yaml';
import { promises as fs } from 'fs';
import path from 'path';

interface QuotaConfig {
  defaults: QuotaDefaults;
  tiers: Record<string, TierConfig>;
  feature_quotas: Record<string, FeatureQuota>;
  rate_limiting: RateLimitConfig;
  alerts: AlertConfig;
  token_calculation: TokenCalculationConfig;
}

interface QuotaDefaults {
  daily_limit: number;
  monthly_limit: number;
  rate_limit: RateLimit;
  model_multipliers: Record<string, number>;
}

interface TierConfig {
  name: string;
  daily_limit: number;
  monthly_limit: number;
  rate_limit: RateLimit;
  allowed_models: string[];
  features: string[];
  restrictions: TierRestrictions;
}

interface RateLimit {
  tokens_per_minute: number;
  requests_per_minute: number;
}

interface TierRestrictions {
  max_context_length: number;
  max_response_length: number;
  no_system_prompts?: boolean;
}

interface FeatureQuota {
  multiplier: number;
  min_tier?: string;
  cache_duration?: number;
}

interface RateLimitConfig {
  enable_burst: boolean;
  burst_multiplier: number;
  burst_duration: number;
  sliding_window: SlidingWindowConfig;
  penalties: PenaltyConfig;
}

interface SlidingWindowConfig {
  enabled: boolean;
  window_size: number;
  precision: number;
}

interface PenaltyConfig {
  first_violation: ViolationAction;
  repeated_violation: ViolationAction;
  abuse_threshold: number;
  abuse_action: string;
  abuse_duration: number;
}

interface ViolationAction {
  action: string;
  duration: number;
}

interface AlertConfig {
  thresholds: AlertThreshold[];
}

interface AlertThreshold {
  percentage: number;
  action: string;
  message: string;
}

interface TokenCalculationConfig {
  include_prompts: boolean;
  include_responses: boolean;
  include_functions: boolean;
  content_multipliers: Record<string, number>;
}

interface TokenUsage {
  userId: string;
  model: string;
  feature?: string;
  tokensUsed: number;
  timestamp: Date;
}

export class QuotaManager {
  private redis: Redis;
  private db: Db;
  private quotaCollection: Collection<any>;
  private config: QuotaConfig;
  
  constructor(
    redisUrl: string,
    mongoUrl: string,
    private configPath: string = path.join(__dirname, '../config/quotas.yaml')
  ) {
    this.redis = new Redis(redisUrl);
    this.initializeMongo(mongoUrl);
  }
  
  private async initializeMongo(mongoUrl: string): Promise<void> {
    const client = new MongoClient(mongoUrl);
    await client.connect();
    this.db = client.db('token_meter');
    this.quotaCollection = this.db.collection('quota_usage');
    
    // Create indexes
    await this.quotaCollection.createIndex({ userId: 1, timestamp: -1 });
    await this.quotaCollection.createIndex({ timestamp: 1 }, { 
      expireAfterSeconds: 90 * 24 * 60 * 60 // 90 days
    });
  }
  
  async loadConfig(): Promise<void> {
    const configContent = await fs.readFile(this.configPath, 'utf8');
    this.config = yaml.load(configContent) as QuotaConfig;
    console.log('✅ Quota configuration loaded successfully');
  }
  
  async checkQuota(
    userId: string,
    model: string,
    tokensRequested: number,
    feature?: string,
    userTier: string = 'free'
  ): Promise<{ allowed: boolean; reason?: string; remaining?: number }> {
    try {
      // Get user's tier config
      const tierConfig = this.config.tiers[userTier] || this.config.tiers.free;
      
      // Check if model is allowed for tier
      if (!this.isModelAllowed(model, tierConfig)) {
        return { allowed: false, reason: `Model ${model} not allowed for ${userTier} tier` };
      }
      
      // Check if feature is allowed for tier
      if (feature && !this.isFeatureAllowed(feature, tierConfig)) {
        return { allowed: false, reason: `Feature ${feature} not allowed for ${userTier} tier` };
      }
      
      // Calculate actual tokens with multipliers
      const actualTokens = this.calculateActualTokens(tokensRequested, model, feature);
      
      // Check rate limits
      const rateLimitCheck = await this.checkRateLimit(userId, actualTokens, tierConfig.rate_limit);
      if (!rateLimitCheck.allowed) {
        return rateLimitCheck;
      }
      
      // Check daily quota
      const dailyCheck = await this.checkDailyQuota(userId, actualTokens, tierConfig.daily_limit);
      if (!dailyCheck.allowed) {
        return dailyCheck;
      }
      
      // Check monthly quota
      const monthlyCheck = await this.checkMonthlyQuota(userId, actualTokens, tierConfig.monthly_limit);
      if (!monthlyCheck.allowed) {
        return monthlyCheck;
      }
      
      // All checks passed
      return { 
        allowed: true, 
        remaining: Math.min(dailyCheck.remaining!, monthlyCheck.remaining!)
      };
      
    } catch (error) {
      console.error('❌ Error checking quota:', error);
      return { allowed: false, reason: 'Internal error checking quota' };
    }
  }
  
  async recordUsage(usage: TokenUsage): Promise<void> {
    try {
      // Record in Redis for real-time tracking
      const dayKey = this.getDayKey(usage.userId);
      const monthKey = this.getMonthKey(usage.userId);
      const minuteKey = this.getMinuteKey(usage.userId);
      
      await Promise.all([
        this.redis.incrby(dayKey, usage.tokensUsed),
        this.redis.expire(dayKey, 86400), // Expire after 1 day
        this.redis.incrby(monthKey, usage.tokensUsed),
        this.redis.expire(monthKey, 2592000), // Expire after 30 days
        this.redis.incrby(minuteKey, usage.tokensUsed),
        this.redis.expire(minuteKey, 60), // Expire after 1 minute
      ]);
      
      // Record in MongoDB for persistence and analytics
      await this.quotaCollection.insertOne({
        ...usage,
        timestamp: usage.timestamp || new Date()
      });
      
      // Check for alerts
      await this.checkAlerts(usage.userId);
      
    } catch (error) {
      console.error('❌ Error recording usage:', error);
      throw error;
    }
  }
  
  private isModelAllowed(model: string, tierConfig: TierConfig): boolean {
    if (tierConfig.allowed_models.includes('*')) return true;
    return tierConfig.allowed_models.includes(model);
  }
  
  private isFeatureAllowed(feature: string, tierConfig: TierConfig): boolean {
    if (tierConfig.features.includes('*')) return true;
    return tierConfig.features.includes(feature);
  }
  
  private calculateActualTokens(tokens: number, model: string, feature?: string): number {
    let actualTokens = tokens;
    
    // Apply model multiplier
    const modelMultiplier = this.config.defaults.model_multipliers[model] || 1.0;
    actualTokens *= modelMultiplier;
    
    // Apply feature multiplier
    if (feature && this.config.feature_quotas[feature]) {
      actualTokens *= this.config.feature_quotas[feature].multiplier;
    }
    
    return Math.ceil(actualTokens);
  }
  
  private async checkRateLimit(
    userId: string, 
    tokens: number, 
    limit: RateLimit
  ): Promise<{ allowed: boolean; reason?: string }> {
    const minuteKey = this.getMinuteKey(userId);
    const currentUsage = await this.redis.get(minuteKey);
    const used = parseInt(currentUsage || '0');
    
    if (used + tokens > limit.tokens_per_minute) {
      await this.recordViolation(userId, 'rate_limit');
      return { 
        allowed: false, 
        reason: `Rate limit exceeded: ${limit.tokens_per_minute} tokens per minute`
      };
    }
    
    return { allowed: true };
  }
  
  private async checkDailyQuota(
    userId: string, 
    tokens: number, 
    limit: number
  ): Promise<{ allowed: boolean; reason?: string; remaining?: number }> {
    const dayKey = this.getDayKey(userId);
    const currentUsage = await this.redis.get(dayKey);
    const used = parseInt(currentUsage || '0');
    const remaining = limit - used;
    
    if (used + tokens > limit) {
      return { 
        allowed: false, 
        reason: `Daily quota exceeded: ${limit} tokens per day`,
        remaining: Math.max(0, remaining)
      };
    }
    
    return { allowed: true, remaining: remaining - tokens };
  }
  
  private async checkMonthlyQuota(
    userId: string, 
    tokens: number, 
    limit: number
  ): Promise<{ allowed: boolean; reason?: string; remaining?: number }> {
    const monthKey = this.getMonthKey(userId);
    const currentUsage = await this.redis.get(monthKey);
    const used = parseInt(currentUsage || '0');
    const remaining = limit - used;
    
    if (used + tokens > limit) {
      return { 
        allowed: false, 
        reason: `Monthly quota exceeded: ${limit} tokens per month`,
        remaining: Math.max(0, remaining)
      };
    }
    
    return { allowed: true, remaining: remaining - tokens };
  }
  
  private async recordViolation(userId: string, type: string): Promise<void> {
    const violationKey = `violations:${userId}:${type}`;
    const count = await this.redis.incr(violationKey);
    await this.redis.expire(violationKey, 3600); // Reset after 1 hour
    
    // Apply penalties based on violation count
    if (count === 1) {
      // First violation - throttle
      await this.applyPenalty(userId, this.config.rate_limiting.penalties.first_violation);
    } else if (count < this.config.rate_limiting.penalties.abuse_threshold) {
      // Repeated violation - suspend
      await this.applyPenalty(userId, this.config.rate_limiting.penalties.repeated_violation);
    } else {
      // Abuse - ban
      await this.applyPenalty(userId, {
        action: this.config.rate_limiting.penalties.abuse_action,
        duration: this.config.rate_limiting.penalties.abuse_duration
      });
    }
  }
  
  private async applyPenalty(userId: string, penalty: ViolationAction): Promise<void> {
    const penaltyKey = `penalty:${userId}`;
    await this.redis.set(penaltyKey, penalty.action, 'EX', penalty.duration);
    
    // Log penalty for audit
    await this.quotaCollection.insertOne({
      type: 'penalty',
      userId,
      action: penalty.action,
      duration: penalty.duration,
      timestamp: new Date()
    });
  }
  
  private async checkAlerts(userId: string): Promise<void> {
    const dayKey = this.getDayKey(userId);
    const currentUsage = await this.redis.get(dayKey);
    const used = parseInt(currentUsage || '0');
    
    // Get user's tier to determine limit
    const userTier = await this.getUserTier(userId);
    const tierConfig = this.config.tiers[userTier] || this.config.tiers.free;
    const limit = tierConfig.daily_limit;
    
    const percentage = (used / limit) * 100;
    
    for (const threshold of this.config.alerts.thresholds) {
      if (percentage >= threshold.percentage) {
        await this.sendAlert(userId, threshold);
      }
    }
  }
  
  private async sendAlert(userId: string, threshold: AlertThreshold): Promise<void> {
    // In a real implementation, this would send notifications
    console.log(`🔔 Alert for user ${userId}: ${threshold.message}`);
    
    // Record alert
    await this.quotaCollection.insertOne({
      type: 'alert',
      userId,
      action: threshold.action,
      message: threshold.message,
      percentage: threshold.percentage,
      timestamp: new Date()
    });
  }
  
  private async getUserTier(userId: string): Promise<string> {
    // In a real implementation, this would fetch from user database
    return 'free';
  }
  
  private getDayKey(userId: string): string {
    const date = new Date().toISOString().split('T')[0];
    return `quota:day:${userId}:${date}`;
  }
  
  private getMonthKey(userId: string): string {
    const date = new Date();
    const month = `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}`;
    return `quota:month:${userId}:${month}`;
  }
  
  private getMinuteKey(userId: string): string {
    const date = new Date();
    const minute = `${date.toISOString().split('T')[0]}-${date.getHours()}-${date.getMinutes()}`;
    return `quota:minute:${userId}:${minute}`;
  }
  
  // Admin functions
  async grantTemporaryBoost(
    userId: string, 
    multiplier: number, 
    duration: number
  ): Promise<void> {
    const boostKey = `boost:${userId}`;
    await this.redis.set(boostKey, multiplier, 'EX', duration);
    
    await this.quotaCollection.insertOne({
      type: 'boost_granted',
      userId,
      multiplier,
      duration,
      timestamp: new Date()
    });
  }
  
  async getUsageReport(userId: string, days: number = 30): Promise<any> {
    const startDate = new Date();
    startDate.setDate(startDate.getDate() - days);
    
    const usage = await this.quotaCollection.aggregate([
      {
        $match: {
          userId,
          timestamp: { $gte: startDate },
          type: { $ne: 'alert' }
        }
      },
      {
        $group: {
          _id: {
            date: { $dateToString: { format: '%Y-%m-%d', date: '$timestamp' } },
            model: '$model'
          },
          totalTokens: { $sum: '$tokensUsed' },
          count: { $sum: 1 }
        }
      },
      {
        $sort: { '_id.date': 1 }
      }
    ]).toArray();
    
    return usage;
  }
}

// Export singleton instance
export const quotaManager = new QuotaManager(
  process.env.REDIS_URL || 'redis://localhost:6379',
  process.env.MONGODB_URL || 'mongodb://localhost:27017',
  process.env.QUOTA_CONFIG_PATH
);